use std::sync::{Arc, Mutex};

use hollow::{
  sketch::{FrameData, Sketch},
  wgpu::{
    bind::BindGroupWithLayout, buffer::Buffer, controller::WGPUController,
  },
};
use wgpu::{RenderPipeline, ShaderModuleDescriptor, TextureView};

pub(crate) struct RunConfig {
  pub(crate) wgsl: String,
  pub(crate) fragment_entry: String,
  pub(crate) vertex_entry: String,
  pub(crate) triangles: u32,
}

pub(crate) struct UserSketchInner {
  triangles: u32,
  primary_bind_group: BindGroupWithLayout,
  time_buffer: Buffer<f32>,
  dimensions_buffer: Buffer<[f32; 2]>,
  render_pipeline: Option<RenderPipeline>,
}

pub(crate) struct UserSketch {
  inner: Option<UserSketchInner>,
  queued_config: Arc<Mutex<Option<RunConfig>>>,
}
impl UserSketch {
  pub(crate) fn new(config: Arc<Mutex<Option<RunConfig>>>) -> Self {
    Self {
      inner: None,
      queued_config: config,
    }
  }
  fn update_config(&mut self, config: RunConfig, wgpu: &WGPUController) {
    let Some(inner) = &mut self.inner else {
      return;
    };
    inner.triangles = config.triangles;
    inner.render_pipeline = Some(
      wgpu
        .build_render_pipeline()
        .add_bind_group_layout(&inner.primary_bind_group.layout)
        .build_with_shader_entry_points(
          &wgpu.shader(ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(
              &config.wgsl,
            )),
          }),
          Some(&config.vertex_entry),
          Some(Some(&config.fragment_entry)),
        ),
    );
  }
}

impl Sketch for UserSketch {
  fn init(&mut self, wgpu: &WGPUController) {
    let time_buffer = wgpu.buffer(0.);
    let dimensions_buffer = wgpu.buffer([0., 0.]);
    let primary_bind_group = wgpu
      .build_bind_group_with_layout()
      .with_uniform_buffer_entry(&dimensions_buffer)
      .with_uniform_buffer_entry(&time_buffer)
      .build();
    self.inner = Some(UserSketchInner {
      time_buffer,
      dimensions_buffer,
      primary_bind_group,
      render_pipeline: None,
      triangles: 0,
    });
  }

  fn update(
    &mut self,
    wgpu: &WGPUController,
    surface_view: TextureView,
    data: FrameData,
  ) {
    let config = if let Ok(mut queued) = self.queued_config.lock() {
      queued.take()
    } else {
      None
    };

    if let Some(config) = config {
      self.update_config(config, wgpu);
    }
    if let Some(inner) = &mut self.inner
      && let Some(render_pipeline) = &inner.render_pipeline
    {
      wgpu
        .write_buffer(&inner.dimensions_buffer, data.dimensions)
        .write_buffer(&inner.time_buffer, data.t);
      wgpu.with_encoder(|encoder| {
        encoder
          .simple_render_pass(&surface_view)
          .with_bind_groups([&inner.primary_bind_group])
          .with_pipeline(render_pipeline)
          .draw(0..(inner.triangles * 3), 0..1);
      });
    }
  }
}
