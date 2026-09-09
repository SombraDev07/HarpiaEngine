//! Window + frame loop. Platform `cfg` stays in `harpia-rhi`.
//! Default `--frames` is never unbounded (AMD/RADV).

use std::num::NonZeroU32;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use harpia_rhi::{
    create, Backend, Device, DeviceDesc, FrameInfo, Gpu, WindowHandles,
};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub backend: Backend,
    pub validation: bool,
    /// `None` = `--interactive` (until the window closes). Default is 90 — never unbounded by omission.
    pub max_frames: Option<NonZeroU32>,
    pub resize_at: Vec<(u32, u32, u32)>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            title: "harpia".into(),
            width: 1280,
            height: 720,
            backend: Backend::Vulkan,
            validation: true,
            max_frames: NonZeroU32::new(90),
            resize_at: vec![(30, 800, 600), (60, 1280, 720)],
        }
    }
}

impl AppConfig {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut cfg = Self::default();
        let mut it = args.into_iter().skip(1);
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--frames" => {
                    let v = it.next().context("`--frames` needs a value")?;
                    let n: u32 = v.parse().context("`--frames` must be a number")?;
                    cfg.max_frames = NonZeroU32::new(n).or_else(|| {
                        tracing::warn!("`--frames 0` refused; using 90 (never unbounded by default)");
                        NonZeroU32::new(90)
                    });
                }
                "--interactive" | "-i" => {
                    cfg.max_frames = None;
                    cfg.resize_at.clear();
                }
                "--backend" => {
                    let v = it.next().context("`--backend` needs vulkan|null")?;
                    cfg.backend = match v.as_str() {
                        "vulkan" | "vk" => Backend::Vulkan,
                        "null" => Backend::Null,
                        other => anyhow::bail!("unknown backend `{other}`"),
                    };
                }
                "--validation" => {
                    let v = it.next().context("`--validation` needs 0|1|on|off")?;
                    cfg.validation = matches!(v.as_str(), "1" | "on" | "true");
                }
                "--width" => {
                    cfg.width = it.next().context("`--width`")?.parse()?;
                }
                "--height" => {
                    cfg.height = it.next().context("`--height`")?.parse()?;
                }
                "--title" => {
                    cfg.title = it.next().context("`--title`")?;
                }
                "--help" | "-h" => {
                    eprintln!(
                        "harpia sample\n  --frames N        default 90 (gate; window closes)\n  --interactive, -i keep the window open until you close it (AMD: not for overnight)\n  --backend vulkan|null\n  --validation 1|0  (default 1)\n  --width --height --title"
                    );
                    std::process::exit(0);
                }
                other => anyhow::bail!("unknown argument `{other}`"),
            }
        }
        Ok(cfg)
    }
}

pub trait Sample {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()>;
    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()>;
}

pub fn run(config: AppConfig, sample: impl Sample) -> Result<ExitCode> {
    install_tracing();
    install_device_loss_abort();
    if config.max_frames.is_none() {
        tracing::warn!(
            "interactive mode: close the window to quit. On AMD/RADV prefer --frames N; unbounded can GPUVM."
        );
    }

    match config.backend {
        Backend::Null => run_null(config, sample),
        Backend::Vulkan => run_winit(config, sample),
    }
}

fn install_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
}

fn install_device_loss_abort() {
    #[cfg(target_os = "linux")]
    {
        unsafe {
            std::env::set_var("MESA_VK_ABORT_ON_DEVICE_LOSS", "1");
        }
    }
}

fn run_null(config: AppConfig, mut sample: impl Sample) -> Result<ExitCode> {
    let mut gpu = create(&DeviceDesc {
        backend: Backend::Null,
        validation: false,
        app_name: "harpia",
        width: config.width,
        height: config.height,
        window: None,
    })
    .context("null device")?;
    sample.init(&mut gpu)?;
    let max = config.max_frames.map(|n| n.get()).unwrap_or(1);
    for _ in 0..max {
        let info = gpu.begin_frame()?;
        if !info.skipped {
            sample.frame(&mut gpu, info)?;
        }
        gpu.end_frame()?;
    }
    Ok(ExitCode::SUCCESS)
}

fn run_winit(config: AppConfig, sample: impl Sample) -> Result<ExitCode> {
    let event_loop = EventLoop::new().context("event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = WinitApp {
        config,
        sample,
        gpu: None,
        window: None,
        frames_done: 0,
        exit_code: 0,
        ready: false,
    };
    event_loop.run_app(&mut app).context("winit run")?;
    match app.exit_code {
        0 => Ok(ExitCode::SUCCESS),
        _ => Ok(ExitCode::from(app.exit_code as u8)),
    }
}

struct WinitApp<S> {
    config: AppConfig,
    sample: S,
    gpu: Option<Gpu>,
    window: Option<Arc<Window>>,
    frames_done: u32,
    exit_code: i32,
    ready: bool,
}

impl<S: Sample> WinitApp<S> {
    fn boot(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let attrs = Window::default_attributes()
            .with_title(self.config.title.clone())
            .with_inner_size(PhysicalSize::new(self.config.width, self.config.height));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .context("create window")?,
        );
        let size = window.inner_size();
        let display = window
            .display_handle()
            .map_err(|e| anyhow!("display handle: {e}"))?
            .as_raw();
        let window_h = window
            .window_handle()
            .map_err(|e| anyhow!("window handle: {e}"))?
            .as_raw();

        let mut gpu = create(&DeviceDesc {
            backend: self.config.backend,
            validation: self.config.validation,
            app_name: "harpia",
            width: size.width.max(1),
            height: size.height.max(1),
            window: Some(WindowHandles {
                display,
                window: window_h,
            }),
        })
        .context("create gpu")?;
        self.sample.init(&mut gpu)?;
        self.gpu = Some(gpu);
        self.window = Some(window);
        self.ready = true;
        Ok(())
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, err: anyhow::Error) {
        tracing::error!("{err:#}");
        self.exit_code = 1;
        event_loop.exit();
    }

    fn draw(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let gpu = self.gpu.as_mut().context("gpu")?;
        let info = gpu.begin_frame()?;
        if !info.skipped {
            self.sample.frame(gpu, info)?;
        }
        gpu.end_frame()?;
        self.frames_done += 1;

        if let Some(&(at, w, h)) = self
            .config
            .resize_at
            .iter()
            .find(|(at, _, _)| *at == self.frames_done)
        {
            tracing::info!(frame = at, width = w, height = h, "programmatic resize");
            if let Some(window) = &self.window {
                let _ = window.request_inner_size(PhysicalSize::new(w, h));
            }
        }

        if let Some(max) = self.config.max_frames {
            if self.frames_done >= max.get() {
                let errors = gpu.validation_error_count();
                if errors > 0 {
                    anyhow::bail!("validation errors: {errors}");
                }
                tracing::info!(
                    frames = self.frames_done,
                    validation_errors = errors,
                    "sample complete"
                );
                event_loop.exit();
            }
        }
        Ok(())
    }
}

impl<S: Sample> ApplicationHandler for WinitApp<S> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.ready {
            return;
        }
        if let Err(e) = self.boot(event_loop) {
            self.fail(event_loop, e);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.exit_code != 0 {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                if self.config.max_frames.is_some() && self.gpu.is_some() {
                    self.exit_code = 1;
                    tracing::error!("window closed before --frames completed");
                } else if let Some(gpu) = &self.gpu {
                    let errors = gpu.validation_error_count();
                    if errors > 0 {
                        self.exit_code = 1;
                        tracing::error!(validation_errors = errors, "closed with validation errors");
                    } else {
                        tracing::info!(
                            frames = self.frames_done,
                            validation_errors = errors,
                            "interactive close"
                        );
                    }
                }
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    if let Err(e) = gpu.resize(size.width, size.height) {
                        self.fail(event_loop, e.into());
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if !self.ready {
                    return;
                }
                if let Err(e) = self.draw(event_loop) {
                    self.fail(event_loop, e);
                } else if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}
