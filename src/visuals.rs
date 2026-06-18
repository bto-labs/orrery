//! The visual layer: an HDR + bloom camera, soft glowing nuclei driven by agent
//! state, an ambient field of background motes to load the GPU, and all the
//! framerate-independent smoothing / procedural motion that turns bursty data
//! into fluid motion.

use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::Hdr;
use bevy::window::PrimaryWindow;

use crate::Config;
use crate::agent::{AgentStatus, Model};
use crate::sync::LatestSnapshot;

/// Base on-screen size (logical px) of a nucleus at unit activity.
const BASE_NUCLEUS_SIZE: f32 = 70.0;
/// Time constant (seconds) for activity smoothing — bigger = smoother/slower.
const ACTIVITY_TAU: f32 = 0.35;
/// Time constant (seconds) for a pulse flare decaying back to zero.
const FLARE_TAU: f32 = 0.28;

/// Marks the single main camera so we can toggle bloom on it.
#[derive(Component)]
pub struct MainCamera;

/// Tracks runtime render toggles for the overlay + bloom switching.
#[derive(Resource)]
pub struct RenderToggles {
    pub bloom_enabled: bool,
}

/// Shared handle to the procedurally generated soft-glow sprite texture.
#[derive(Resource)]
pub struct GlowTexture(pub Handle<Image>);

/// A glowing agent nucleus. Holds both the `target` values (from data) and the
/// `displayed` values (eased toward the target each frame).
#[derive(Component)]
pub struct Nucleus {
    pub agent_id: u32,
    /// Home position as a fraction of the screen half-extents, so it tracks
    /// resolution / resize without re-layout.
    pub home_norm: Vec2,
    pub velocity: Vec2,
    pub phase: f32,
    pub wobble_seed: f32,
    pub displayed_activity: f32,
    pub target_activity: f32,
    pub status: AgentStatus,
    pub model: Model,
    pub last_pulse_count: u32,
    /// Current flare intensity from a recent discrete pulse (decays to 0).
    pub flare: f32,
}

/// An ambient background mote — cheap, batched glow sprites that exist to load
/// the GPU at 5K (mirrors PLAN.md's ~850-mote reference field).
#[derive(Component)]
pub struct Mote {
    pub velocity: Vec2,
    pub phase: f32,
    pub base_brightness: f32,
}

/// Exponential, framerate-independent ease factor for time step `dt` and time
/// constant `tau`: the fraction of the remaining gap to close this frame.
pub fn ease_factor(dt: f32, tau: f32) -> f32 {
    1.0 - (-dt / tau).exp()
}

/// Build an HDR colour from hue/saturation with a brightness multiplier that
/// can exceed 1.0 (so the camera's bloom pass picks it up as a glow).
fn hdr_color(hue: f32, saturation: f32, brightness: f32) -> Color {
    let lin = Color::hsl(hue, saturation, 0.5).to_linear();
    Color::LinearRgba(LinearRgba::new(
        lin.red * brightness,
        lin.green * brightness,
        lin.blue * brightness,
        1.0,
    ))
}

/// Generate a radial-falloff white sprite: a bright core fading to a soft halo.
/// Tinted per-sprite via `Sprite::color`; bloom does the actual glow.
fn make_glow_image(size: u32) -> Image {
    let mut data = vec![0u8; (size * size * 4) as usize];
    let center = (size as f32 - 1.0) * 0.5;
    let radius = center.max(1.0);
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt() / radius; // 0 centre .. 1 edge
            let t = (1.0 - dist).clamp(0.0, 1.0);
            let alpha = t * t; // soft, bright core with a faint halo
            let i = ((y * size + x) * 4) as usize;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = (alpha * 255.0) as u8;
        }
    }
    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// Half-extents of the visible area in world (logical-pixel) units. The default
/// `Camera2d` projection centres the world on the screen, so this is half the
/// window size. Falls back to a sane default before the window is sized.
fn screen_half_extents(window: Option<&Window>) -> Vec2 {
    match window {
        Some(w) => Vec2::new(w.width(), w.height()) * 0.5,
        None => Vec2::new(960.0, 540.0),
    }
}

/// Spawn `count` motes spread across the screen, drifting slowly.
pub fn spawn_motes(commands: &mut Commands, glow: &Handle<Image>, count: usize, half: Vec2) {
    for _ in 0..count {
        let x = (fastrand::f32() * 2.0 - 1.0) * half.x;
        let y = (fastrand::f32() * 2.0 - 1.0) * half.y;
        let angle = fastrand::f32() * TAU;
        let speed = 6.0 + fastrand::f32() * 16.0;
        let size = 10.0 + fastrand::f32() * 18.0;
        let brightness = 0.25 + fastrand::f32() * 0.8;
        commands.spawn((
            Sprite {
                image: glow.clone(),
                color: hdr_color(205.0, 0.3, brightness),
                custom_size: Some(Vec2::splat(size)),
                ..default()
            },
            Transform::from_xyz(x, y, 0.0),
            Mote {
                velocity: Vec2::new(angle.cos(), angle.sin()) * speed,
                phase: fastrand::f32() * TAU,
                base_brightness: brightness,
            },
        ));
    }
}

/// Startup: HDR + bloom camera, glow texture, nuclei, and the mote field.
pub fn setup_scene(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    config: Res<Config>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    // HDR camera. `Hdr` enables the intermediate HDR target that bloom needs;
    // TonyMcMapface desaturates bright colours toward white. Bloom itself is
    // added conditionally so it can be measured on/off from the CLI.
    let camera = commands
        .spawn((
            Camera2d,
            Camera {
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                ..default()
            },
            Hdr,
            Tonemapping::TonyMcMapface,
            DebandDither::Enabled,
            MainCamera,
        ))
        .id();
    if config.bloom {
        commands.entity(camera).insert(Bloom::default());
    }

    let glow = images.add(make_glow_image(128));

    // Nuclei laid out on a phyllotaxis (sunflower) spiral for an organic feel.
    let n = config.agents.max(1);
    for i in 0..n {
        let frac = (i as f32 + 0.5) / n as f32;
        let r = frac.sqrt() * 0.42;
        let theta = i as f32 * 2.399_963_2; // golden angle (radians)
        let home_norm = Vec2::new(r * theta.cos(), r * theta.sin());
        commands.spawn((
            Sprite {
                image: glow.clone(),
                color: Color::WHITE,
                custom_size: Some(Vec2::splat(BASE_NUCLEUS_SIZE)),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, 1.0),
            Nucleus {
                agent_id: i as u32,
                home_norm,
                velocity: Vec2::ZERO,
                phase: fastrand::f32() * TAU,
                wobble_seed: fastrand::f32() * TAU,
                displayed_activity: 0.0,
                target_activity: 0.0,
                status: AgentStatus::Idle,
                model: Model::from_index(i),
                last_pulse_count: 0,
                flare: 0.0,
            },
        ));
    }

    let half = screen_half_extents(windows.iter().next());
    spawn_motes(&mut commands, &glow, config.motes, half);

    commands.insert_resource(GlowTexture(glow));
}

/// Pull the latest snapshot onto the nuclei: set targets, and convert any
/// change in `pulse_count` into a flare. Runs after `read_latest_snapshot`.
pub fn apply_targets(latest: Res<LatestSnapshot>, mut nuclei: Query<&mut Nucleus>) {
    if latest.0.is_empty() {
        return;
    }
    for mut nuc in &mut nuclei {
        let id = nuc.agent_id;
        if let Some(state) = latest.0.iter().find(|s| s.id == id) {
            nuc.target_activity = state.activity_level;
            nuc.status = state.status;
            nuc.model = state.model;
            if state.pulse_count != nuc.last_pulse_count {
                nuc.flare = (nuc.flare + 1.0).min(1.6);
                nuc.last_pulse_count = state.pulse_count;
            }
        }
    }
}

/// Per-frame nucleus animation: ease displayed activity toward target, decay
/// flares, breathe/pulse/wobble by status, and move via spring-to-home + drift
/// + noise. All easing is framerate-independent.
pub fn animate_nuclei(
    time: Res<Time>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut nuclei: Query<(&mut Nucleus, &mut Transform, &mut Sprite)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let t = time.elapsed_secs();
    let half = screen_half_extents(windows.iter().next());
    let activity_ease = ease_factor(dt, ACTIVITY_TAU);
    let flare_decay = (-dt / FLARE_TAU).exp();

    for (mut nuc, mut transform, mut sprite) in &mut nuclei {
        nuc.displayed_activity += (nuc.target_activity - nuc.displayed_activity) * activity_ease;
        nuc.flare *= flare_decay;

        let act = nuc.displayed_activity;
        let status = nuc.status;

        // Breathing (idle) vs pulsing (active) vs jitter (error).
        let speed = match status {
            AgentStatus::Idle => 0.8,
            AgentStatus::Active => 2.5 + act * 3.0,
            AgentStatus::Error => 7.0,
        };
        let breathe = ((t * speed + nuc.phase).sin() * 0.5 + 0.5) * 0.18;

        let size = BASE_NUCLEUS_SIZE * (0.55 + act * 1.4 + breathe + nuc.flare * 0.9);
        sprite.custom_size = Some(Vec2::splat(size));

        let (hue, sat) = match status {
            AgentStatus::Error => (0.0, 0.95),
            _ => (nuc.model.hue(), 0.85),
        };
        let brightness = 1.0 + act * 4.0 + breathe * 2.0 + nuc.flare * 9.0;
        sprite.color = hdr_color(hue, sat, brightness);

        // Procedural motion: damped spring toward home + wander + error wobble.
        let home = nuc.home_norm * (half * 2.0);
        let pos = transform.translation.truncate();
        let mut accel = (home - pos) * 6.0;
        accel += Vec2::new(fastrand::f32() - 0.5, fastrand::f32() - 0.5) * 260.0;
        if status == AgentStatus::Error {
            accel += Vec2::new(
                (t * 28.0 + nuc.wobble_seed).sin(),
                (t * 31.0 + nuc.wobble_seed).cos(),
            ) * 500.0;
        }
        let damp = (1.0 - 4.0 * dt).clamp(0.0, 1.0);
        let mut vel = nuc.velocity * damp + accel * dt;
        if vel.length() > 1500.0 {
            vel = vel.normalize() * 1500.0;
        }
        nuc.velocity = vel;
        let new_pos = pos + vel * dt;
        transform.translation.x = new_pos.x;
        transform.translation.y = new_pos.y;
    }
}

/// Per-frame mote drift: slow movement, wrap at the edges, gentle twinkle.
pub fn animate_motes(
    time: Res<Time>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut motes: Query<(&Mote, &mut Transform, &mut Sprite)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let t = time.elapsed_secs();
    let half = screen_half_extents(windows.iter().next());
    let bx = half.x + 40.0;
    let by = half.y + 40.0;

    for (mote, mut transform, mut sprite) in &mut motes {
        let mut p = transform.translation.truncate() + mote.velocity * dt;
        if p.x > bx {
            p.x = -bx;
        } else if p.x < -bx {
            p.x = bx;
        }
        if p.y > by {
            p.y = -by;
        } else if p.y < -by {
            p.y = by;
        }
        transform.translation.x = p.x;
        transform.translation.y = p.y;

        let twinkle = (t * 0.5 + mote.phase).sin() * 0.5 + 0.5;
        sprite.color = hdr_color(205.0, 0.3, mote.base_brightness * (0.6 + twinkle * 0.8));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_factor_is_in_unit_range_and_monotonic_in_dt() {
        let tau = 0.35;
        let small = ease_factor(0.001, tau);
        let big = ease_factor(0.1, tau);
        assert!(small > 0.0 && small < 1.0);
        assert!(big > small && big < 1.0);
    }

    #[test]
    fn ease_factor_zero_dt_closes_nothing() {
        assert_eq!(ease_factor(0.0, 0.35), 0.0);
    }

    #[test]
    fn smoothing_glides_toward_target_without_overshoot() {
        // A burst sets target to 1.0; displayed should approach it smoothly,
        // monotonically, and never overshoot.
        let mut displayed = 0.0f32;
        let target = 1.0f32;
        let factor = ease_factor(1.0 / 60.0, ACTIVITY_TAU);
        let mut prev = displayed;
        for _ in 0..240 {
            displayed += (target - displayed) * factor;
            assert!(displayed <= target + 1e-6, "overshot target");
            assert!(displayed >= prev, "not monotonic");
            prev = displayed;
        }
        assert!(displayed > 0.99, "did not converge: {displayed}");
    }
}
