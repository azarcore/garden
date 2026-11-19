//! Bevy app: ground, surfaces, walls, buildings and plants from a
//! `garden_scene::Scene`, with an orbit camera. View only.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::light::CascadeShadowConfigBuilder;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::winit::{UpdateMode, WinitSettings};
use garden_core::model::{PlantCategory, PlantSymbol};
use garden_scene::{CameraStart, MeshData, PlantInstance, Scene, SurfaceKind};
use wasm_bindgen::prelude::*;

/// The next scene to show, handed over from JavaScript.
static PENDING: Mutex<Option<Scene>> = Mutex::new(None);
/// Whether the Bevy app is already running.
static STARTED: AtomicBool = AtomicBool::new(false);

const SKY: Color = Color::srgb(0.874, 0.913, 0.886);
const TRUNK: Color = Color::srgb(0.43, 0.31, 0.22);
const MIN_RADIUS: f32 = 2.0;
const MAX_RADIUS: f32 = 400.0;
/// Keep the camera this far above the horizon (radians) so it never goes underground.
const MIN_PITCH: f32 = 0.08;
const MAX_PITCH: f32 = 1.5;

fn parse(design_json: &str) -> Result<Scene, JsValue> {
    let doc = garden_core::io::from_json(design_json)
        .map_err(|e| JsValue::from_str(&format!("Couldn't read the design: {e}")))?;
    Ok(garden_scene::build(&doc))
}

/// Start the viewer in the canvas matching `canvas_selector` (once), showing `design_json`.
#[wasm_bindgen]
pub fn open(canvas_selector: &str, design_json: &str) -> Result<(), JsValue> {
    let scene = parse(design_json)?;
    if let Ok(mut p) = PENDING.lock() {
        *p = Some(scene);
    }
    if !STARTED.swap(true, Ordering::SeqCst) {
        start(canvas_selector);
    }
    Ok(())
}

/// Show another snapshot in the running viewer (re-opening 3D after edits).
#[wasm_bindgen]
pub fn show(design_json: &str) -> Result<(), JsValue> {
    let scene = parse(design_json)?;
    if let Ok(mut p) = PENDING.lock() {
        *p = Some(scene);
    }
    Ok(())
}

fn start(canvas_selector: &str) {
    App::new()
        .add_plugins(
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
            brightness: 700.0,
            ..default()
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (apply_pending, orbit_controls).chain())
        .run();
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
    commands.spawn((Camera3d::default(), t, orbit));
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            illuminance: 9_000.0,
            ..default()
        },
        // low sun from the south-west
        Transform::from_xyz(-30.0, 36.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
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
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
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
) {
    let Some(shapes) = shapes else { return };
    let Some(scene) = PENDING.lock().ok().and_then(|mut p| p.take()) else {
        return;
    };
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
    let mut part = |mesh: &Handle<Mesh>, mat: Handle<StandardMaterial>, centre_y: f32, size: Vec3| {
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat),
            Transform::from_translation(base + Vec3::Y * centre_y).with_scale(size),
            GardenPart,
        ));
    };
    match p.category {
        PlantCategory::Tree => {
            let trunk_h = 0.35 * h;
            let trunk_w = (0.12 * s).min(0.5);
            part(&shapes.cylinder, shapes.trunk.clone(), trunk_h / 2.0, Vec3::new(trunk_w, trunk_h, trunk_w));
            if p.symbol == PlantSymbol::Conifer {
                part(&shapes.cone, material, h - 0.4 * h, Vec3::new(s, 0.8 * h, s));
            } else {
                part(&shapes.sphere, material, h - 0.35 * h, Vec3::new(s, 0.7 * h, s));
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
        let right = t.right().as_vec3();
        let forward = Vec3::new(t.forward().x, 0.0, t.forward().z).normalize_or_zero();
        let k = orbit.radius * 0.0015;
        orbit.target += (-right * d.x + forward * d.y) * k;
        changed = true;
    } else if d != Vec2::ZERO && buttons.pressed(MouseButton::Left) {
        orbit.yaw -= d.x * 0.006;
        orbit.pitch = (orbit.pitch + d.y * 0.006).clamp(MIN_PITCH, MAX_PITCH);
        changed = true;
    }
    if scroll.delta.y != 0.0 {
        let k = match scroll.unit {
            MouseScrollUnit::Line => 0.12,
            MouseScrollUnit::Pixel => 0.0015,
        };
        orbit.radius = (orbit.radius * (-scroll.delta.y * k).exp()).clamp(MIN_RADIUS, MAX_RADIUS);
        changed = true;
    }
    if changed {
        orbit.place(&mut t);
    }
}
