use std::collections::BTreeMap;

use b_spline::so3bspline::SO3Bspline;
use bevy::math::cubic_splines::CubicBSpline;
use bevy::prelude::*;
use bevy_panorbit_camera::{PanOrbitCamera, PanOrbitCameraPlugin};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Pose {
    rvec: [f64; 3],
    tvec: [f64; 3],
}

#[derive(Resource)]
struct Bspline {
    pub tvecs: CubicCurve<Vec3>,
    pub rvecs: SO3Bspline<4>,
    pub total: f32,
    pub current: f32,
    pub t_start: u64,
    pub t_end: u64,
}

fn main() {
    let contents = std::fs::read_to_string("examples/poses.json")
        .expect("Should have been able to read the file");
    let poses: BTreeMap<u64, Pose> = serde_json::from_str(&contents).unwrap();
    let mut rvecs_with_t: Vec<_> = poses.iter().map(|(k, v)| (*k, v.rvec)).collect();
    rvecs_with_t.sort_by(|a, b| a.0.cmp(&b.0));
    let mut pose_vec: Vec<_> = poses.iter().map(|(k, v)| (*k, v)).collect();
    pose_vec.sort_by(|a, b| a.0.cmp(&b.0));
    let (ts_ns, rvecs): (Vec<_>, Vec<_>) = rvecs_with_t.iter().map(|a| (a.0, a.1)).unzip();
    let tvec_spline = CubicBSpline::new(
        pose_vec
            .iter()
            .map(|(_, p)| Vec3::new(p.tvec[0] as f32, p.tvec[1] as f32, p.tvec[2] as f32)),
    )
    .to_curve()
    .unwrap();

    let spacing_ns = 250_000_000;

    let rotation_bspline = SO3Bspline::<4>::from_rotation_vectors(&ts_ns, &rvecs, spacing_ns);
    let i = 100;
    println!("{:?} ", rvecs[i]);
    println!("{}", rotation_bspline.get_rotation(ts_ns[i]).log());

    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(PanOrbitCameraPlugin)
        .insert_resource(Bspline {
            tvecs: tvec_spline,
            rvecs: rotation_bspline,
            total: (ts_ns.len() - 3) as f32,
            current: 0.0,
            t_start: ts_ns[0],
            t_end: ts_ns[ts_ns.len() - 1],
        })
        .add_systems(Startup, setup)
        .add_systems(Update, update)
        .run();
}

#[derive(Component)]
struct Cube;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // circular base
    commands.spawn((
        Mesh3d(meshes.add(Circle::new(40.0))),
        MeshMaterial3d(materials.add(Color::WHITE)),
        // Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
    ));

    // cube
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(Color::srgb_u8(124, 144, 255))),
        Transform::from_xyz(0.0, 0.0, 0.5),
        Cube,
    ));
    // light
    commands.spawn((
        PointLight {
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));
    // camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(-2.5, 4.5, 9.0).looking_at(Vec3::ZERO, Vec3::Y),
        PanOrbitCamera::default(),
    ));
}

fn update(mut bspline: ResMut<Bspline>, mut query: Query<&mut Transform, With<Cube>>) {
    // bspline.current
    let mut transform = query.single_mut().unwrap();

    let tvec = bspline.tvecs.position(bspline.current) / 10.0;
    let timestamp_ns = bspline.t_start
        + ((bspline.t_end - bspline.t_start) as f32 * bspline.current / bspline.total) as u64;
    let quat = bspline.rvecs.get_rotation(timestamp_ns).to_vec().cast();

    *transform = Transform::from_isometry(Isometry3d {
        rotation: Quat::from_xyzw(quat.x, quat.y, quat.z, quat.w),
        translation: tvec.into(),
    });
    // transform.translation = tvec / 10.0;
    bspline.current += 1.0;
    if bspline.current > bspline.total {
        bspline.current -= bspline.total
    }
}
