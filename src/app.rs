use hollow::{
  sketch::{FrameData, Sketch},
  wgpu::{
    bind::BindGroupWithLayout, buffer::Buffer, controller::WGPUController,
  },
};
use wgpu::{RenderPipeline, ShaderModuleDescriptor, TextureView};

pub(crate) struct RunConfig {
  pub(crate) fragment_entry: String,
  pub(crate) vertex_entry: String,
  pub(crate) triangles: u32,
}

pub(crate) struct UserSketchInner {
  triangles: u32,
  primary_bind_group: BindGroupWithLayout,
  time_buffer: Buffer<f32>,
  dimensions_buffer: Buffer<[f32; 2]>,
  render_pipeline: RenderPipeline,
}

pub(crate) enum UserSketch {
  Uninitialized(String, RunConfig),
  Initialized(UserSketchInner),
}
impl UserSketch {
  pub(crate) fn new(wgsl_source: String, config: RunConfig) -> Self {
    Self::Uninitialized(wgsl_source, config)
  }
}

impl Sketch for UserSketch {
  fn init(&mut self, wgpu: &WGPUController) {
    let Self::Uninitialized(wgsl, config) = self else {
      panic!()
    };
    let time_buffer = wgpu.buffer(0.);
    let dimensions_buffer = wgpu.buffer([0., 0.]);
    let primary_bind_group = wgpu
      .build_bind_group_with_layout()
      .with_uniform_buffer_entry(&dimensions_buffer)
      .with_uniform_buffer_entry(&time_buffer)
      .build();
    let render_pipeline = wgpu
      .build_render_pipeline()
      .add_bind_group_layout(&primary_bind_group.layout)
      .build_with_shader_entry_points(
        &wgpu.shader(ShaderModuleDescriptor {
          label: None,
          source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(wgsl)),
        }),
        Some(&config.vertex_entry),
        Some(Some(&config.fragment_entry)),
      );
    *self = Self::Initialized(UserSketchInner {
      time_buffer,
      dimensions_buffer,
      primary_bind_group,
      render_pipeline,
      triangles: config.triangles,
    });
  }

  fn update(
    &mut self,
    wgpu: &WGPUController,
    surface_view: TextureView,
    data: FrameData,
  ) {
    if let Self::Initialized(inner) = self {
      wgpu
        .write_buffer(&inner.dimensions_buffer, data.dimensions)
        .write_buffer(&inner.time_buffer, data.t);
      wgpu.with_encoder(|encoder| {
        encoder
          .simple_render_pass(&surface_view)
          .with_bind_groups([&inner.primary_bind_group])
          .with_pipeline(&inner.render_pipeline)
          .draw(0..(inner.triangles * 3), 0..1);
      });
    }
  }
}
