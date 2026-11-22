//! Bevy app: ground, surfaces, walls, buildings and plants from a
//! `loma_scene::Scene`, with an orbit camera. View only.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::gestures::{Finger, Gesture, Taps, gesture};
use crate::sunlight::{Light, default_light, light_for};
use bevy::asset::RenderAssetUsages;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::light::CascadeShadowConfigBuilder;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::window::RequestRedraw;
use bevy::winit::{
    EventLoopProxy, EventLoopProxyWrapper, UpdateMode, WinitSettings, WinitUserEvent,
};
use loma_core::model::{PlantCategory, PlantSymbol};
use loma_scene::{CameraStart, MeshData, PlantInstance, Scene, SurfaceKind};
use wasm_bindgen::prelude::*;

/// The next scene to show, handed over from JavaScript.
static PENDING: Mutex<Option<Scene>> = Mutex::new(None);
/// Wakes the (otherwise idle) viewer when a new snapshot arrives.
static WAKER: Mutex<Option<EventLoopProxy<WinitUserEvent>>> = Mutex::new(None);
/// Set if the viewer panicked (e.g. the GPU lacks what Bevy needs).
static FAILED: AtomicBool = AtomicBool::new(false);
/// Double taps, timed by the browser's own event timestamps (frames can lag
/// well behind input, which would make a quick tap look like a long press).
static TAPS: Mutex<Option<Taps>> = Mutex::new(None);
/// Set by a double tap; the next frame puts the view back where it started.
static RESET: AtomicBool = AtomicBool::new(false);
/// Whether the Bevy app is already running.
static STARTED: AtomicBool = AtomicBool::new(false);
/// The next lighting to show (a new sun position), handed over from JavaScript.
static PENDING_LIGHT: Mutex<Option<Light>> = Mutex::new(None);

const SKY: Color = Color::srgb(
    crate::sunlight::DAY_SKY[0],
    crate::sunlight::DAY_SKY[1],
    crate::sunlight::DAY_SKY[2],
);
/// How far from the garden the sun light sits (it's directional; only the
/// direction matters).
const SUN_DISTANCE: f32 = 60.0;
const TRUNK: Color = Color::srgb(0.43, 0.31, 0.22);
const MIN_RADIUS: f32 = 2.0;
const MAX_RADIUS: f32 = 400.0;
/// Keep the camera this far above the horizon (radians) so it never goes underground.
const MIN_PITCH: f32 = 0.08;
const MAX_PITCH: f32 = 1.5;
/// Frames to keep drawing after a new scene arrives: new meshes and
/// materials reach the GPU a frame or two later, and the viewer otherwise only
/// redraws on input.
const SETTLE_FRAMES: u8 = 10;

/// Hand a new scene to the viewer and wake it so it shows straight away.
fn hand_over(scene: Scene) {
    if let Ok(mut p) = PENDING.lock() {
        *p = Some(scene);
    }
    wake();
}

/// Run a frame soon, even if the viewer is idle.
fn wake() {
    if let Ok(w) = WAKER.lock()
        && let Some(proxy) = w.as_ref()
    {
        let _ = proxy.send_event(WinitUserEvent::WakeUp);
    }
}

fn remember_waker(proxy: Res<EventLoopProxyWrapper>) {
    if let Ok(mut w) = WAKER.lock() {
        *w = Some((**proxy).clone());
    }
}

fn parse(design_json: &str) -> Result<Scene, JsValue> {
    let doc = loma_core::io::from_json(design_json)
        .map_err(|e| JsValue::from_str(&format!("Couldn't read the design: {e}")))?;
    Ok(loma_scene::build(&doc))
}

/// Start the viewer in the canvas matching `canvas_selector` (once), showing `design_json`.
#[wasm_bindgen]
pub fn open(canvas_selector: &str, design_json: &str) -> Result<(), JsValue> {
    hand_over(parse(design_json)?);
    if !STARTED.swap(true, Ordering::SeqCst) {
        start(canvas_selector);
    }
    Ok(())
}

/// True once the viewer has crashed; the page then shows a message.
#[wasm_bindgen]
pub fn has_failed() -> bool {
    FAILED.load(Ordering::SeqCst)
}

/// Show another snapshot in the running viewer (re-opening 3D after edits).
#[wasm_bindgen]
pub fn show(design_json: &str) -> Result<(), JsValue> {
    hand_over(parse(design_json)?);
    Ok(())
}

/// Light the garden from a sun at `bearing` (degrees clockwise from the top
/// of the plan) and `altitude` (degrees above the horizon).
#[wasm_bindgen]
pub fn set_sun(bearing: f32, altitude: f32) {
    if bearing.is_finite() && altitude.is_finite() {
        hand_over_light(light_for(bearing, altitude));
    }
}

/// Light the garden as when it has no location: a low south-west sun.
#[wasm_bindgen]
pub fn default_sun() {
    hand_over_light(default_light());
}

fn hand_over_light(light: Light) {
    if let Ok(mut p) = PENDING_LIGHT.lock() {
        *p = Some(light);
    }
    wake();
}

/// The directional light that plays the sun.
#[derive(Component)]
struct Sun;

/// Put a new sun in place: direction, strength, shadows, ambient and sky.
fn apply_light(
    mut sun: Query<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut sky: ResMut<ClearColor>,
    mut settling: ResMut<Settling>,
) {
    let Some(light) = PENDING_LIGHT.lock().ok().and_then(|mut p| p.take()) else {
        return;
    };
    for (mut directional, mut transform) in &mut sun {
        directional.illuminance = light.sun;
        directional.shadow_maps_enabled = light.shadows;
        let at = Vec3::from_array(light.dir_to_sun) * SUN_DISTANCE;
        // straight overhead "looking at" the origin needs another up direction
        let up = if light.dir_to_sun[1] > 0.999 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        *transform = Transform::from_translation(at).looking_at(Vec3::ZERO, up);
    }
    ambient.brightness = light.ambient;
    sky.0 = Color::srgb(light.sky[0], light.sky[1], light.sky[2]);
    settling.0 = SETTLE_FRAMES;
}

/// Watch the canvas for double taps (touch only).
fn listen_for_taps(canvas_selector: &str) {
    use wasm_bindgen::JsCast;
    let Some(canvas) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.query_selector(canvas_selector).ok().flatten())
    else {
        return;
    };
    #[derive(Clone, Copy)]
    enum Kind {
        Down,
        Up,
        Cancel,
    }
    let on_tap = |kind: Kind| {
        Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |e: web_sys::PointerEvent| {
            if e.pointer_type() != "touch" {
                return;
            }
            let Ok(mut taps) = TAPS.lock() else { return };
            let taps = taps.get_or_insert_with(Taps::default);
            let (id, t, x, y) = (
                e.pointer_id() as u64,
                e.time_stamp(),
                e.client_x() as f32,
                e.client_y() as f32,
            );
            match kind {
                Kind::Down => {
                    if e.is_primary() {
                        // no other finger is down: forget any lift we missed
                        taps.all_lifted();
                    }
                    taps.press(id, t, x, y);
                }
                Kind::Up => {
                    if taps.release(id, t, x, y) {
                        RESET.store(true, Ordering::SeqCst);
                        wake();
                    }
                }
                Kind::Cancel => taps.cancel(id),
            }
        })
    };
    for (event, kind) in [
        ("pointerdown", Kind::Down),
        ("pointerup", Kind::Up),
        ("pointercancel", Kind::Cancel),
    ] {
        let listener = on_tap(kind);
        let _ = canvas.add_event_listener_with_callback(event, listener.as_ref().unchecked_ref());
        listener.forget();
    }
}

fn start(canvas_selector: &str) {
    listen_for_taps(canvas_selector);
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    canvas: Some(canvas_selector.to_string()),
                    fit_canvas_to_parent: true,
                    prevent_default_event_handling: true,
                    ..default()
                }),
                ..default()
            })
            .set(bevy::log::LogPlugin {
                level: bevy::log::Level::WARN,
                ..default()
            }),
    )
    // Redraw on input only (and now and then, to pick up a new snapshot),
    // so a closed 3D view costs next to nothing.
    .insert_resource(WinitSettings {
        focused_mode: UpdateMode::reactive(Duration::from_millis(500)),
        unfocused_mode: UpdateMode::reactive_low_power(Duration::from_secs(1)),
    })
    .insert_resource(ClearColor(SKY))
    .insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: 770.0,
        ..default()
    })
    .add_systems(Startup, (setup, remember_waker))
    .init_resource::<Settling>()
    .init_resource::<StartView>()
    .init_resource::<LastFingers>()
    .add_systems(
        Update,
        (
            apply_pending,
            apply_light,
            orbit_controls,
            touch_controls,
            keep_drawing,
        )
            .chain(),
    );
    // after the plugins: Bevy installs its own panic hook while adding them
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        FAILED.store(true, Ordering::SeqCst);
        previous(info);
    }));
    app.run();
}

/// Redraws still owed after the last new scene.
#[derive(Resource, Default)]
struct Settling(u8);

fn keep_drawing(mut settling: ResMut<Settling>, mut redraw: MessageWriter<RequestRedraw>) {
    if settling.0 > 0 {
        settling.0 -= 1;
        redraw.write(RequestRedraw);
    }
}

/// Everything built from a scene, removed when the next one arrives.
#[derive(Component)]
struct GardenPart;

/// Where the camera looks from: around `target`, `radius` metres away.
#[derive(Component)]
struct Orbit {
    target: Vec3,
    radius: f32,
    yaw: f32,
    pitch: f32,
}

impl Orbit {
    fn from_start(c: CameraStart) -> Orbit {
        Orbit {
            target: Vec3::from_array(c.target),
            radius: c.radius.clamp(MIN_RADIUS, MAX_RADIUS),
            yaw: c.yaw,
            pitch: c.pitch.clamp(MIN_PITCH, MAX_PITCH),
        }
    }

    /// Turn around the target by a drag of (dx, dy) px.
    fn turn(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.006;
        self.pitch = (self.pitch + dy * 0.006).clamp(MIN_PITCH, MAX_PITCH);
    }

    /// Move across the garden by a drag of (dx, dy) px, as seen from `t`.
    fn pan(&mut self, dx: f32, dy: f32, t: &Transform) {
        let right = t.right().as_vec3();
        let forward = Vec3::new(t.forward().x, 0.0, t.forward().z).normalize_or_zero();
        let k = self.radius * 0.0015;
        self.target += (-right * dx + forward * dy) * k;
    }

    /// Move closer (`factor` < 1) or further away.
    fn zoom(&mut self, factor: f32) {
        self.radius = (self.radius * factor).clamp(MIN_RADIUS, MAX_RADIUS);
    }

    fn place(&self, t: &mut Transform) {
        let dir = Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        );
        *t = Transform::from_translation(self.target + dir * self.radius)
            .looking_at(self.target, Vec3::Y);
    }
}

/// Unit-sized shapes shared by every plant (scaled per plant).
#[derive(Resource)]
struct PlantShapes {
    sphere: Handle<Mesh>,
    cylinder: Handle<Mesh>,
    cone: Handle<Mesh>,
    trunk: Handle<StandardMaterial>,
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let orbit = Orbit {
        target: Vec3::ZERO,
        radius: 20.0,
        yaw: 0.0,
        pitch: 0.6,
    };
    let mut t = Transform::default();
    orbit.place(&mut t);
    commands.spawn((
        Camera3d::default(),
        // the default tone mapper needs lookup-table textures (0.7 MB more to
        // download); Reinhard with the lights a touch brighter looks the same
        Tonemapping::Reinhard,
        t,
        orbit,
    ));
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            illuminance: 9_900.0,
            ..default()
        },
        // low sun from the south-west until a real one is set
        Transform::from_xyz(-30.0, 36.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
        Sun,
        CascadeShadowConfigBuilder {
            maximum_distance: 250.0,
            ..default()
        }
        .build(),
    ));
    commands.insert_resource(PlantShapes {
        sphere: meshes.add(Sphere::new(0.5).mesh().uv(24, 16)),
        cylinder: meshes.add(Cylinder::new(0.5, 1.0)),
        cone: meshes.add(Cone::new(0.5, 1.0)),
        trunk: materials.add(StandardMaterial {
            base_color: TRUNK,
            perceptual_roughness: 0.95,
            ..default()
        }),
    });
}

fn to_mesh(m: &MeshData) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, m.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, m.normals.clone())
    .with_inserted_indices(Indices::U32(m.indices.clone()))
}

fn surface_material(m: &MeshData) -> StandardMaterial {
    let [r, g, b, a] = m.color;
    let mut mat = StandardMaterial {
        base_color: Color::srgba(r, g, b, a),
        perceptual_roughness: 0.9,
        ..default()
    };
    if m.kind == SurfaceKind::Water {
        mat.perceptual_roughness = 0.15;
        mat.metallic = 0.1;
        mat.reflectance = 0.6;
    }
    mat
}

/// Take a new scene if one is waiting and rebuild everything from it.
#[allow(clippy::too_many_arguments)]
fn apply_pending(
    mut commands: Commands,
    parts: Query<Entity, With<GardenPart>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    shapes: Option<Res<PlantShapes>>,
    mut camera: Query<(&mut Orbit, &mut Transform)>,
    mut settling: ResMut<Settling>,
    mut start_view: ResMut<StartView>,
) {
    let Some(shapes) = shapes else { return };
    let Some(scene) = PENDING.lock().ok().and_then(|mut p| p.take()) else {
        return;
    };
    settling.0 = SETTLE_FRAMES;
    for e in &parts {
        commands.entity(e).despawn();
    }
    for m in &scene.meshes {
        if m.indices.is_empty() {
            continue;
        }
        commands.spawn((
            Mesh3d(meshes.add(to_mesh(m))),
            MeshMaterial3d(materials.add(surface_material(m))),
            Transform::default(),
            GardenPart,
        ));
    }
    let mut plant_materials: HashMap<[u8; 4], Handle<StandardMaterial>> = HashMap::new();
    for p in &scene.plants {
        let key = p.color.map(|v| (v * 255.0).round() as u8);
        let material = plant_materials
            .entry(key)
            .or_insert_with(|| {
                let [r, g, b, _] = p.color;
                materials.add(StandardMaterial {
                    base_color: Color::srgb(r, g, b),
                    perceptual_roughness: 0.95,
                    ..default()
                })
            })
            .clone();
        spawn_plant(&mut commands, &shapes, material, p);
    }
    if let (Some(start), Ok((mut orbit, mut t))) = (scene.camera, camera.single_mut()) {
        start_view.0 = Some(start);
        *orbit = Orbit::from_start(start);
        orbit.place(&mut t);
    }
}

/// A plant as simple shapes: trees get a trunk and a round (or conical)
/// crown, grasses a cone, everything else a mound.
fn spawn_plant(
    commands: &mut Commands,
    shapes: &PlantShapes,
    material: Handle<StandardMaterial>,
    p: &PlantInstance,
) {
    let base = Vec3::from_array(p.position);
    let (s, h) = (p.spread.max(0.05), p.height.max(0.05));
    let mut part =
        |mesh: &Handle<Mesh>, mat: Handle<StandardMaterial>, centre_y: f32, size: Vec3| {
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat),
                Transform::from_translation(base + Vec3::Y * centre_y).with_scale(size),
                GardenPart,
            ));
        };
    match p.category {
        PlantCategory::Tree | PlantCategory::FruitTree => {
            let trunk_h = 0.35 * h;
            let trunk_w = (0.12 * s).min(0.5);
            part(
                &shapes.cylinder,
                shapes.trunk.clone(),
                trunk_h / 2.0,
                Vec3::new(trunk_w, trunk_h, trunk_w),
            );
            if p.symbol == PlantSymbol::Conifer {
                part(
                    &shapes.cone,
                    material,
                    h - 0.4 * h,
                    Vec3::new(s, 0.8 * h, s),
                );
            } else {
                part(
                    &shapes.sphere,
                    material,
                    h - 0.35 * h,
                    Vec3::new(s, 0.7 * h, s),
                );
            }
        }
        PlantCategory::Grass => part(&shapes.cone, material, h / 2.0, Vec3::new(s, h, s)),
        PlantCategory::Shrub | PlantCategory::Climber | PlantCategory::Other => {
            part(&shapes.sphere, material, h / 2.0, Vec3::new(s, h, s));
        }
        PlantCategory::Perennial
        | PlantCategory::Annual
        | PlantCategory::Herb
        | PlantCategory::Vegetable => {
            // a mound: the lower part of the shape sits below the ground
            part(&shapes.sphere, material, 0.35 * h, Vec3::new(s, 1.3 * h, s));
        }
    }
}

/// Left-drag turns around the garden, right-drag (or Shift-drag) moves
/// across it, the wheel zooms.
fn orbit_controls(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut camera: Query<(&mut Orbit, &mut Transform)>,
) {
    let Ok((mut orbit, mut t)) = camera.single_mut() else {
        return;
    };
    let mut changed = false;
    let d = motion.delta;
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let panning = buttons.pressed(MouseButton::Right)
        || buttons.pressed(MouseButton::Middle)
        || (shift && buttons.pressed(MouseButton::Left));
    if d != Vec2::ZERO && panning {
        orbit.pan(d.x, d.y, &t);
        changed = true;
    } else if d != Vec2::ZERO && buttons.pressed(MouseButton::Left) {
        orbit.turn(d.x, d.y);
        changed = true;
    }
    if scroll.delta.y != 0.0 {
        let k = match scroll.unit {
            MouseScrollUnit::Line => 0.12,
            MouseScrollUnit::Pixel => 0.0006,
        };
        orbit.zoom((-scroll.delta.y * k).exp());
        changed = true;
    }
    if changed {
        orbit.place(&mut t);
    }
}

/// Where the camera started for the garden on show (double-tap goes back there).
#[derive(Resource, Default)]
struct StartView(Option<CameraStart>);

/// The fingers down when `touch_controls` last ran.
#[derive(Resource, Default)]
struct LastFingers(Vec<Finger>);

/// One finger turns around the garden, two fingers pinch to zoom and move
/// across it, a double tap goes back to the starting view.
fn touch_controls(
    touches: Res<Touches>,
    start_view: Res<StartView>,
    mut last: ResMut<LastFingers>,
    mut camera: Query<(&mut Orbit, &mut Transform)>,
) {
    let Ok((mut orbit, mut t)) = camera.single_mut() else {
        return;
    };
    let reset = RESET.swap(false, Ordering::SeqCst);
    if reset && let Some(start) = start_view.0 {
        *orbit = Orbit::from_start(start);
        orbit.place(&mut t);
        return;
    }
    // compare with the fingers as this system last saw them (Bevy's
    // `previous_position` repeats the last move in frames without new input);
    // a finger pressed or lifted since makes the lists differ, so nothing jumps
    let now: Vec<Finger> = touches
        .iter()
        .map(|tc| Finger {
            id: tc.id(),
            x: tc.position().x,
            y: tc.position().y,
        })
        .collect();
    let prev = std::mem::replace(&mut last.0, now.clone());
    match gesture(&prev, &now) {
        Gesture::Still => return,
        Gesture::Orbit { dx, dy } => orbit.turn(dx, dy),
        Gesture::Pinch { zoom, dx, dy } => {
            orbit.zoom(zoom);
            orbit.pan(dx, dy, &t);
        }
    }
    orbit.place(&mut t);
}
