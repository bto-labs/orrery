//! On-screen diagnostics overlay plus the one-time startup log of the render
//! stack (wgpu backend, adapter, driver, and the true physical resolution).

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::window::PrimaryWindow;

use crate::visuals::{Mote, Nucleus, RenderToggles};

/// Static render-stack facts captured once at startup, shown in the overlay.
#[derive(Resource, Default)]
pub struct RenderInfo {
    pub backend: String,
    pub adapter: String,
}

/// Marks the diagnostics overlay text entity.
#[derive(Component)]
pub struct DiagText;

/// Spawn the top-left diagnostics overlay.
pub fn setup_overlay(mut commands: Commands) {
    commands.spawn((
        Text::new("orrery — starting…"),
        TextFont {
            font_size: 18.0,
            ..default()
        },
        TextColor(Color::srgb(0.78, 0.88, 1.0)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(14.0),
            left: Val::Px(16.0),
            ..default()
        },
        DiagText,
    ));
}

/// Log the backend / adapter / driver / resolution exactly once, after the
/// window has settled into fullscreen (so the resolution is the true physical
/// size, not the pre-fullscreen request). Also stashes backend + adapter for
/// the overlay.
pub fn report_render_info_once(
    mut done: Local<bool>,
    time: Res<Time>,
    adapter_info: Option<Res<RenderAdapterInfo>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut info: ResMut<RenderInfo>,
) {
    if *done || time.elapsed_secs() < 0.8 {
        return;
    }
    let Some(adapter) = adapter_info else {
        return;
    };
    let Some(window) = windows.iter().next() else {
        return;
    };
    let res = &window.resolution;
    let (pw, ph) = (res.physical_width(), res.physical_height());
    if pw == 0 || ph == 0 {
        return;
    }

    let a = &adapter.0; // WgpuWrapper<AdapterInfo>, derefs to AdapterInfo
    info.backend = format!("{:?}", a.backend);
    info.adapter = a.name.clone();

    info!("== orrery render stack ==");
    info!("  backend     : {:?}", a.backend);
    info!("  adapter     : {}", a.name);
    info!("  driver      : {}", a.driver);
    info!("  driver_info : {}", a.driver_info);
    info!(
        "  resolution  : {pw}x{ph} (physical)  scale {:.3}",
        res.scale_factor()
    );
    info!("  logical     : {:.0}x{:.0}", res.width(), res.height());

    *done = true;
}

/// Update the overlay each frame with live FPS, frame time, entity counts,
/// backend, adapter, resolution, and the bloom toggle state.
pub fn update_overlay(
    diagnostics: Res<DiagnosticsStore>,
    info: Res<RenderInfo>,
    toggles: Res<RenderToggles>,
    windows: Query<&Window, With<PrimaryWindow>>,
    nuclei: Query<(), With<Nucleus>>,
    motes: Query<(), With<Mote>>,
    mut text: Query<&mut Text, With<DiagText>>,
) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let frame_ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);

    let n_nuclei = nuclei.iter().count();
    let n_motes = motes.iter().count();

    let resolution = match windows.iter().next() {
        Some(w) => format!(
            "{}x{} @ {:.2}x",
            w.resolution.physical_width(),
            w.resolution.physical_height(),
            w.resolution.scale_factor()
        ),
        None => "—".to_string(),
    };

    let backend = if info.backend.is_empty() {
        "…"
    } else {
        &info.backend
    };
    let adapter = if info.adapter.is_empty() {
        "…"
    } else {
        &info.adapter
    };

    if let Ok(mut text) = text.single_mut() {
        text.0 = format!(
            "orrery POC\n\
             FPS {fps:>5.0}   {frame_ms:>5.2} ms\n\
             backend  {backend}\n\
             adapter  {adapter}\n\
             res      {resolution}\n\
             agents {n_nuclei}   motes {n_motes}   entities {}\n\
             bloom {}   [B] bloom   [+/-] motes   [Esc] quit",
            n_nuclei + n_motes,
            if toggles.bloom_enabled { "ON " } else { "off" },
        );
    }
}
