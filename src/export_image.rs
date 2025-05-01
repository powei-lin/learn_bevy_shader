use bevy::{
    ecs::{
        query::QueryItem,
        system::{SystemParamItem, lifetimeless::SRes},
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderSet,
        camera::CameraUpdateSystem,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        graph::CameraDriverLabel,
        render_asset::{PrepareAssetError, RenderAsset, RenderAssetPlugin, RenderAssetUsages},
        render_graph::RenderGraph,
        render_graph::{Node, NodeRunError, RenderGraphContext, RenderLabel},
        render_resource::*,
        renderer::RenderDevice,
    },
    render::{render_asset::RenderAssets, renderer::RenderContext, texture::GpuImage},
};
use futures::channel::oneshot;
use image::{DynamicImage, RgbaImage};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
// use bevy
// use wgpu::Maintain;

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub struct ImageExportLabel;

pub struct ImageExportNode;
impl Node for ImageExportNode {
    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        for (_, source) in world
            .resource::<RenderAssets<GpuImageExportSource>>()
            .iter()
        {
            if let Some(gpu_image) = world
                .resource::<RenderAssets<GpuImage>>()
                .get(&source.source_handle)
            {
                render_context.command_encoder().copy_texture_to_buffer(
                    gpu_image.texture.as_image_copy(),
                    ImageCopyBuffer {
                        buffer: &source.buffer,
                        layout: ImageDataLayout {
                            offset: 0,
                            bytes_per_row: Some(source.padded_bytes_per_row),
                            rows_per_image: None,
                        },
                    },
                    source.source_size,
                );
            }
        }

        Ok(())
    }
}

#[derive(Asset, Reflect, Clone, Default)]
pub struct ImageExportSource(pub Handle<Image>);

impl From<Handle<Image>> for ImageExportSource {
    fn from(value: Handle<Image>) -> Self {
        Self(value)
    }
}

#[derive(Component, Clone)]
pub struct ImageExportSettings {
    /// The directory that image files will be saved to.
    pub output_dir: String,
    /// The image file extension. E.g. "png", "jpeg", or "exr".
    pub extension: String,

    pub enabled: bool,
}

pub struct GpuImageExportSource {
    pub buffer: Buffer,
    pub source_handle: Handle<Image>,
    pub source_size: Extent3d,
    pub bytes_per_row: u32,
    pub padded_bytes_per_row: u32,
}

impl RenderAsset for GpuImageExportSource {
    type SourceAsset = ImageExportSource;
    type Param = (SRes<RenderDevice>, SRes<RenderAssets<GpuImage>>);

    fn asset_usage(_: &Self::SourceAsset) -> RenderAssetUsages {
        RenderAssetUsages::RENDER_WORLD
    }

    fn prepare_asset(
        source_asset: Self::SourceAsset,
        (device, images): &mut SystemParamItem<Self::Param>,
    ) -> Result<Self, PrepareAssetError<Self::SourceAsset>> {
        let gpu_image = images.get(&source_asset.0).unwrap();

        let size = gpu_image.texture.size();
        let format = &gpu_image.texture_format;
        let bytes_per_row =
            (size.width / format.block_dimensions().0) * format.block_copy_size(None).unwrap();
        let padded_bytes_per_row =
            RenderDevice::align_copy_bytes_per_row(bytes_per_row as usize) as u32;

        let source_size = gpu_image.texture.size();

        Ok(GpuImageExportSource {
            buffer: device.create_buffer(&BufferDescriptor {
                label: Some("Image Export Buffer"),
                size: (source_size.height * padded_bytes_per_row) as u64,
                usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
            source_handle: source_asset.0,
            source_size,
            bytes_per_row,
            padded_bytes_per_row,
        })
    }

    fn byte_len(_: &Self::SourceAsset) -> Option<usize> {
        None
    }
}

#[derive(Component, Clone)]
pub struct ImageExportStartFrame(u64);

impl Default for ImageExportSettings {
    fn default() -> Self {
        Self {
            output_dir: "out".into(),
            extension: "png".into(),
            enabled: false,
        }
    }
}

impl ExtractComponent for ImageExportSettings {
    type QueryData = (
        &'static Self,
        &'static ImageExport,
        &'static ImageExportStartFrame,
    );
    type QueryFilter = ();
    type Out = (Self, ImageExport, ImageExportStartFrame);

    fn extract_component(
        (settings, export, start_frame): QueryItem<'_, Self::QueryData>,
    ) -> Option<Self::Out> {
        Some((
            settings.clone(),
            ImageExport(export.0.clone_weak()),
            start_frame.clone(),
        ))
    }
}

fn setup_exporters(
    mut commands: Commands,
    exporters: Query<Entity, (With<ImageExportSettings>, Without<ImageExportStartFrame>)>,
    mut frame_id: Local<u64>,
) {
    *frame_id = frame_id.wrapping_add(1);
    for entity in &exporters {
        commands
            .entity(entity)
            .insert(ImageExportStartFrame(*frame_id));
    }
}

#[derive(Component, Clone, Default)]
#[require(ImageExportSettings)]
pub struct ImageExport(pub Handle<ImageExportSource>);

#[derive(Default, Clone, Resource)]
pub struct ExportThreads {
    pub count: Arc<AtomicUsize>,
}

impl ExportThreads {
    /// Blocks the main thread until all frames have been saved successfully.
    pub fn finish(&self) {
        while self.count.load(Ordering::SeqCst) > 0 {
            std::thread::sleep(std::time::Duration::from_secs_f32(0.25));
        }
    }
}

fn save_buffer_to_disk(
    export_bundles: Query<(&ImageExport, &ImageExportSettings, &ImageExportStartFrame)>,
    sources: Res<RenderAssets<GpuImageExportSource>>,
    render_device: Res<RenderDevice>,
    export_threads: Res<ExportThreads>,
    mut frame_id: Local<u64>,
) {
    *frame_id = frame_id.wrapping_add(1);
    for (export, settings, start_frame) in &export_bundles {
        if !settings.enabled {
            continue;
        }
        if let Some(gpu_source) = sources.get(&export.0) {
            let mut image_bytes = {
                let slice = gpu_source.buffer.slice(..);

                {
                    let (mapping_tx, mapping_rx) = oneshot::channel();

                    render_device.map_buffer(&slice, MapMode::Read, move |res| {
                        mapping_tx.send(res).unwrap();
                    });

                    render_device.poll(Maintain::Wait);
                    futures_lite::future::block_on(mapping_rx).unwrap().unwrap();
                }

                slice.get_mapped_range().to_vec()
            };

            gpu_source.buffer.unmap();

            let settings = settings.clone();
            let frame_id = *frame_id - start_frame.0 + 1;
            let bytes_per_row = gpu_source.bytes_per_row as usize;
            let padded_bytes_per_row = gpu_source.padded_bytes_per_row as usize;
            let source_size = gpu_source.source_size;
            let export_threads = export_threads.clone();

            export_threads.count.fetch_add(1, Ordering::SeqCst);
            std::thread::spawn(move || {
                if bytes_per_row != padded_bytes_per_row {
                    let mut unpadded_bytes =
                        Vec::<u8>::with_capacity(source_size.height as usize * bytes_per_row);

                    for padded_row in image_bytes.chunks(padded_bytes_per_row) {
                        unpadded_bytes.extend_from_slice(&padded_row[..bytes_per_row]);
                    }

                    image_bytes = unpadded_bytes;
                }

                let path = format!(
                    "{}/{:05}.{}",
                    settings.output_dir, frame_id, settings.extension
                );

                std::fs::create_dir_all(&settings.output_dir)
                    .expect("Output path could not be created");

                save_rgb(image_bytes, &source_size, path.as_str());

                export_threads.count.fetch_sub(1, Ordering::SeqCst);
            });
        }
    }
}

fn save_rgb(image_bytes: Vec<u8>, source_size: &Extent3d, path: &str) {
    if let Some(img) = RgbaImage::from_raw(source_size.width, source_size.height, image_bytes) {
        DynamicImage::ImageRgba8(img)
            .to_rgb8()
            .save(path)
            .expect("fail to save");
    } else {
        println!("failed");
    }
}

/// Plugin enabling the generation of image sequences.
#[derive(Default)]
pub struct ImageExportPlugin {
    pub threads: ExportThreads,
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub enum ImageExportSystems {
    SetupImageExport,
    SetupImageExportFlush,
}

impl Plugin for ImageExportPlugin {
    fn build(&self, app: &mut App) {
        use ImageExportSystems::*;

        app.configure_sets(
            PostUpdate,
            (SetupImageExport, SetupImageExportFlush)
                .chain()
                .before(CameraUpdateSystem),
        )
        .register_type::<ImageExportSource>()
        .init_asset::<ImageExportSource>()
        .register_asset_reflect::<ImageExportSource>()
        .add_plugins((
            RenderAssetPlugin::<GpuImageExportSource>::default(),
            ExtractComponentPlugin::<ImageExportSettings>::default(),
        ))
        .add_systems(
            PostUpdate,
            (
                setup_exporters.in_set(SetupImageExport),
                apply_deferred.in_set(SetupImageExportFlush),
            ),
        );

        let render_app = app.sub_app_mut(RenderApp);

        render_app
            .insert_resource(self.threads.clone())
            .add_systems(
                Render,
                save_buffer_to_disk
                    .after(RenderSet::Render)
                    .before(RenderSet::Cleanup),
            );

        let mut graph = render_app
            .world_mut()
            .get_resource_mut::<RenderGraph>()
            .unwrap();

        graph.add_node(ImageExportLabel, ImageExportNode);
        graph.add_node_edge(CameraDriverLabel, ImageExportLabel);
    }
}
