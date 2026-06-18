//! orrery — Stage-0 proof of concept.
//!
//! A GPU-accelerated ambient visualization of (synthetic) Claude Code agent
//! activity. This POC de-risks the two riskiest assumptions of the full build
//! (see `PLAN.md`):
//!
//!  1. Bevy/wgpu can hold ~60 fps borderless-fullscreen at native 5K on Wayland
//!     with an HDR bloom pass and hundreds–thousands of animated sprites; and
//!  2. a background data source can feed per-agent state into the render loop
//!     across a lock-free `triple_buffer`, smoothing bursty changes into fluid
//!     motion.
//!
//! Everything beyond that (real RabbitMQ/REST/Mimir, tokio, Rapier physics,
//! idle/D-Bus integration) is intentionally out of scope for this stage.

mod diagnostics;
mod ingest;
mod visuals;

use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::window::screenshot::{Screenshot, save_to_disk};
use bevy::window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode};

use visuals::{GlowTexture, MainCamera, Mote, RenderToggles};

/// Runtime configuration, from env vars and/or CLI flags.
#[derive(Resource, Clone, Copy, Debug)]
pub struct Config {
    pub agents: usize,
    pub motes: usize,
    pub seed: u64,
    /// Whether bloom is enabled at startup (toggleable later with `B`).
    pub bloom: bool,
    /// VSync on (cap to refresh) or off (uncapped, to measure raw throughput).
    pub vsync: bool,
    /// Milliseconds of no activity before a session is considered idle.
    pub idle_ms: u64,
    /// Milliseconds after last activity before a session is despawned.
    pub despawn_ms: u64,
    /// Maximum concurrent sessions tracked (cap enforcement deferred to Plan 2).
    pub max_agents: usize,
    /// Run with synthetic data source (true by default in Stage 1).
    pub synthetic: bool,
}

/// How many motes to add/remove per keypress.
const MOTE_STEP: usize = 100;

/// When set, capture one framebuffer screenshot to this path after warmup, then
/// exit. Used for headless visual verification (no compositor permission needed).
#[derive(Resource)]
pub struct ScreenshotMode(pub Option<String>);

/// Parse config from `ORRERY_AGENTS` / `ORRERY_MOTES` / `ORRERY_SEED` env vars,
/// overridable by `--agents N` / `--motes M` CLI flags. Also returns an optional
/// screenshot output path (`--screenshot <path>` / `ORRERY_SCREENSHOT`).
fn parse_config() -> (Config, Option<String>) {
    fn env_usize(key: &str, default: usize) -> usize {
        std::env::var(key)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(default)
    }
    fn env_u64(key: &str, default: u64) -> u64 {
        std::env::var(key)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(default)
    }

    let mut agents = env_usize("ORRERY_AGENTS", 16);
    let mut motes = env_usize("ORRERY_MOTES", 800);
    let seed = std::env::var("ORRERY_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x00C0_FFEE);
    // Bloom on by default; ORRERY_BLOOM=0/false (or --no-bloom) starts it off.
    let mut bloom = !matches!(
        std::env::var("ORRERY_BLOOM").ok().as_deref(),
        Some("0") | Some("false")
    );
    // VSync on by default; ORRERY_VSYNC=0/false (or --no-vsync) uncaps it.
    let mut vsync = !matches!(
        std::env::var("ORRERY_VSYNC").ok().as_deref(),
        Some("0") | Some("false")
    );
    let mut screenshot = std::env::var("ORRERY_SCREENSHOT").ok();
    let mut idle_ms = env_u64("ORRERY_IDLE_MS", 30_000);
    let mut despawn_ms = env_u64("ORRERY_DESPAWN_MS", 120_000);
    let mut max_agents = env_usize("ORRERY_MAX_AGENTS", 64);
    // Synthetic is on by default in Stage 1; --synthetic is the explicit
    // dev-mode opt-in named in the spec.
    let mut synthetic = true;

    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--screenshot" => {
                screenshot = args.get(i + 1).cloned();
                i += 2;
            }
            "--agents" => {
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                    agents = v;
                }
                i += 2;
            }
            "--motes" => {
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                    motes = v;
                }
                i += 2;
            }
            "--no-bloom" => {
                bloom = false;
                i += 1;
            }
            "--no-vsync" => {
                vsync = false;
                i += 1;
            }
            "--idle-ms" => {
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                    idle_ms = v;
                }
                i += 2;
            }
            "--despawn-ms" => {
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                    despawn_ms = v;
                }
                i += 2;
            }
            "--max-agents" => {
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                    max_agents = v;
                }
                i += 2;
            }
            "--synthetic" => {
                synthetic = true;
                i += 1;
            }
            _ => i += 1,
        }
    }

    (
        Config {
            agents: agents.max(1),
            motes,
            seed,
            bloom,
            vsync,
            idle_ms,
            despawn_ms,
            max_agents,
            synthetic,
        },
        screenshot,
    )
}

fn main() {
    let (config, screenshot) = parse_config();

    // Start the tokio ingestion runtime before building the app; fail cleanly
    // rather than panicking if the OS refuses the thread. Stage 1 is
    // synthetic-only; live sources arrive in Plan 2.
    let synthetic = if config.synthetic {
        Some((config.agents, config.seed))
    } else {
        None
    };
    let (receiver, ingest_handle) =
        match ingest::spawn_ingest(config.idle_ms, config.despawn_ms, synthetic) {
            Ok(pair) => pair,
            Err(err) => {
                eprintln!("orrery: failed to start ingestion: {err}");
                std::process::exit(1);
            }
        };

    println!(
        "orrery POC starting — {} agents, {} motes, seed {:#x}, \
         idle_ms {}, despawn_ms {}, max_agents {} (cap enforcement deferred to Plan 2)",
        config.agents,
        config.motes,
        config.seed,
        config.idle_ms,
        config.despawn_ms,
        config.max_agents,
    );

    App::new()
        .add_plugins(
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "orrery".into(),
                    // Borderless — NEVER exclusive — fullscreen on the current
                    // monitor. Wayland no-ops exclusive fullscreen.
                    mode: WindowMode::BorderlessFullscreen(MonitorSelection::Current),
                    present_mode: if config.vsync {
                        PresentMode::AutoVsync
                    } else {
                        PresentMode::AutoNoVsync
                    },
                    ..default()
                }),
                ..default()
            }),
        )
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        // Logs FPS / frame time to the terminal every second, so the sustained
        // numbers can be captured without screenshotting the overlay.
        .add_plugins(LogDiagnosticsPlugin::default())
        .insert_resource(config)
        .insert_resource(receiver)
        .insert_resource(ingest_handle)
        .insert_resource(RenderToggles {
            bloom_enabled: config.bloom,
        })
        .insert_resource(ScreenshotMode(screenshot))
        .init_resource::<ingest::LatestSnapshot>()
        .init_resource::<diagnostics::RenderInfo>()
        .add_systems(Startup, (visuals::setup_scene, diagnostics::setup_overlay))
        .add_systems(
            Update,
            (
                // Data → reconcile → targets → animation, in order, every frame.
                (
                    ingest::read_latest_snapshot,
                    visuals::reconcile_nuclei,
                    visuals::apply_targets,
                    visuals::animate_nuclei,
                )
                    .chain(),
                visuals::animate_motes,
                diagnostics::report_render_info_once,
                diagnostics::update_overlay,
                controls,
                auto_screenshot,
            ),
        )
        .run();
}

/// In `--screenshot` mode: capture the primary window's framebuffer once after
/// warmup (so smoothing/pulses are visible), then exit shortly after. No-op
/// otherwise.
fn auto_screenshot(
    mode: Res<ScreenshotMode>,
    mut frame: Local<u32>,
    mut captured: Local<bool>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = mode.0.as_ref() else {
        return;
    };
    *frame += 1;
    if *frame == 300 && !*captured {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path.clone()));
        *captured = true;
    }
    if *frame >= 420 {
        exit.write(AppExit::Success);
    }
}

/// Runtime controls: Esc quits, B toggles bloom, +/- adjust the mote count.
#[allow(clippy::too_many_arguments)]
fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
    mut toggles: ResMut<RenderToggles>,
    camera: Query<Entity, With<MainCamera>>,
    glow: Res<GlowTexture>,
    motes: Query<Entity, With<Mote>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        info!("[control] Esc pressed — exiting");
        exit.write(AppExit::Success);
    }

    if keys.just_pressed(KeyCode::KeyB)
        && let Ok(cam) = camera.single()
    {
        if toggles.bloom_enabled {
            commands.entity(cam).remove::<Bloom>();
        } else {
            commands.entity(cam).insert(Bloom::default());
        }
        toggles.bloom_enabled = !toggles.bloom_enabled;
        info!(
            "[control] B pressed — bloom now {}",
            if toggles.bloom_enabled { "ON" } else { "OFF" }
        );
    }

    let add = keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd);
    let remove = keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract);

    if add {
        let half = match windows.iter().next() {
            Some(w) => Vec2::new(w.width(), w.height()) * 0.5,
            None => Vec2::new(960.0, 540.0),
        };
        visuals::spawn_motes(&mut commands, &glow.0, MOTE_STEP, half);
        info!(
            "[control] +/= pressed — motes {} -> {}",
            motes.iter().count(),
            motes.iter().count() + MOTE_STEP
        );
    }
    if remove {
        let current = motes.iter().count();
        let removed = MOTE_STEP.min(current);
        for entity in motes.iter().take(MOTE_STEP) {
            commands.entity(entity).despawn();
        }
        info!(
            "[control] -/_ pressed — motes {} -> {}",
            current,
            current - removed
        );
    }
}
