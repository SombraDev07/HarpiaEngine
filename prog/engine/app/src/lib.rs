//! Window + frame loop. Platform `cfg` stays in `harpia-rhi`.
//! Default `--frames` is never unbounded (AMD/RADV).

mod capture;

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use harpia_core::{Input, Key, MouseButton};
pub use harpia_core::{Input as SampleInput, Key as SampleKey, MouseButton as SampleMouseButton};
use harpia_rhi::{
    create, Backend, Device, DeviceDesc, FrameInfo, Gpu, Texture, WindowHandles,
};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};
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
    /// `--capture <prefix>`: after the last frame, dump the sample's
    /// [`Sample::capture_targets`] as `<prefix>.<name>.png`.
    pub capture: Option<PathBuf>,
    /// `--vsync 0` picks the fastest present mode so a frame time can be
    /// measured. On by default: an uncapped window is a hot GPU for nothing.
    pub vsync: bool,
    /// `--stats` prints CPU and GPU timings when the run ends.
    pub stats: bool,
    /// Everything after a bare `--`, untouched, for a sample's own switches.
    ///
    /// Without this the parser rejects any flag it does not know, so a gate that
    /// wants an A/B mode has no way to ask for one.
    pub extra: Vec<String>,
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
            capture: None,
            vsync: true,
            stats: false,
            extra: Vec::new(),
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
                "--capture" => {
                    cfg.capture = Some(PathBuf::from(
                        it.next().context("`--capture` needs a path prefix")?,
                    ));
                }
                "--help" | "-h" => {
                    eprintln!(
                        "harpia sample\n  --frames N        default 90 (gate; window closes)\n  --interactive, -i keep the window open until you close it (AMD: not for overnight)\n  --backend vulkan|null\n  --validation 1|0  (default 1)\n  --capture PREFIX  PNG of each capture target after the last frame\n  --vsync 0|1       0 = fastest present mode, for measuring\n  --stats           CPU and GPU timings at the end\n  -- ARGS...        passed to the sample\n  --width --height --title"
                    );
                    std::process::exit(0);
                }
                "--vsync" => {
                    let v = it.next().context("`--vsync` needs 0|1")?;
                    cfg.vsync = v != "0";
                }
                "--stats" => cfg.stats = true,
                "--" => {
                    cfg.extra.extend(it.by_ref());
                }
                other => anyhow::bail!("unknown argument `{other}`"),
            }
        }
        Ok(cfg)
    }
}

/// Fixed step for `--frames N`. 60 Hz because that is what the samples that
/// animate off `frame_index` already assume.
const GATE_DT: f32 = 1.0 / 60.0;

pub trait Sample {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()>;
    /// Simulation, before the frame is recorded. `dt` is in seconds.
    ///
    /// Under `--frames N` the input is always empty and `dt` is always
    /// `1/60`, so a gate is reproducible whatever the machine or the user is
    /// doing. Live input and a real clock only happen in `--interactive`.
    fn update(&mut self, _input: &Input, _dt: f32) {}
    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()>;
    /// Depois do último frame, fora de qualquer frame aberto.
    ///
    /// É aqui que um gate de medição lê resultados de volta: `read_texture`
    /// espera pelo device e é ilegal a meio de um frame. Devolver `Err` faz o
    /// sample sair com código ≠ 0, que é o que torna uma medição um gate.
    fn finish(&mut self, _gpu: &mut Gpu) -> Result<()> {
        Ok(())
    }

    /// Render targets `--capture` should write out. Empty = nothing to dump.
    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        Vec::new()
    }
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
        vsync: config.vsync,
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
        input: Input::new(),
        last_frame: None,
        cpu_ms: Vec::new(),
        gpu_stats: harpia_rhi::GpuStats::default(),
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
    input: Input,
    last_frame: Option<std::time::Instant>,
    /// CPU milliseconds per frame, for `--stats`.
    cpu_ms: Vec<f32>,
    /// The most recent GPU stats seen. Timestamps arrive a frame or two late, so
    /// the last complete one is the one worth reporting.
    gpu_stats: harpia_rhi::GpuStats,
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
            vsync: self.config.vsync,
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

    /// Print what the run cost.
    ///
    /// The first frames are warm-up -- pipeline creation, first-use uploads,
    /// shader caches -- so they are dropped rather than allowed to dominate the
    /// average and flatter or slander the steady state.
    fn report_stats(&mut self) {
        const WARMUP: usize = 8;
        let mut ms = self.cpu_ms.clone();
        if ms.len() > WARMUP * 2 {
            ms.drain(..WARMUP);
        }
        if ms.is_empty() {
            return;
        }
        ms.sort_by(f32::total_cmp);
        let avg: f32 = ms.iter().sum::<f32>() / ms.len() as f32;
        let pick = |q: f32| ms[((ms.len() - 1) as f32 * q) as usize];
        tracing::info!(
            frames = ms.len(),
            cpu_avg_ms = format!("{avg:.2}"),
            cpu_min_ms = format!("{:.2}", ms[0]),
            cpu_p99_ms = format!("{:.2}", pick(0.99)),
            cpu_max_ms = format!("{:.2}", ms[ms.len() - 1]),
            vsync = self.config.vsync,
            "cpu"
        );
        let g = &self.gpu_stats;
        if g.frame_ms > 0.0 {
            tracing::info!(
                gpu_frame_ms = format!("{:.3}", g.frame_ms),
                draws = g.draws,
                dispatches = g.dispatches,
                triangles = g.triangles,
                "gpu"
            );
            for (label, ms) in &g.passes {
                tracing::info!(pass = label, ms = format!("{ms:.3}"), "gpu pass");
            }
        } else {
            tracing::warn!("no GPU timestamps: the queue may not support them");
        }
    }

    fn draw(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        // A gate is a measurement, so it gets a fixed step and no input at all.
        // Wall-clock dt would make every `--capture` depend on how busy the
        // machine was, and the whole verification loop rests on those being
        // reproducible.
        let dt = if self.config.max_frames.is_some() {
            GATE_DT
        } else {
            let now = std::time::Instant::now();
            let dt = self
                .last_frame
                .map_or(GATE_DT, |t| now.duration_since(t).as_secs_f32());
            self.last_frame = Some(now);
            // A breakpoint or a dragged window must not teleport the camera.
            dt.clamp(1.0 / 1000.0, 0.1)
        };
        self.sample.update(&self.input, dt);
        let frame_started = std::time::Instant::now();

        let gpu = self.gpu.as_mut().context("gpu")?;
        let info = gpu.begin_frame()?;
        if !info.skipped {
            self.sample.frame(gpu, info)?;
        }
        gpu.end_frame()?;
        let stats = gpu.take_stats();
        if stats.frame_ms > 0.0 {
            self.gpu_stats = stats;
        }
        self.input.end_frame();
        self.cpu_ms.push(frame_started.elapsed().as_secs_f32() * 1000.0);
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
                let gpu = self.gpu.as_mut().context("gpu")?;
                self.sample.finish(gpu)?;
                if let Some(prefix) = self.config.capture.clone() {
                    let targets = self.sample.capture_targets();
                    let gpu = self.gpu.as_mut().context("gpu")?;
                    capture::capture_all(gpu, &prefix, &targets)?;
                }
                if self.config.stats {
                    self.report_stats();
                }
                let gpu = self.gpu.as_mut().context("gpu")?;
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

    /// Raw motion, not `CursorMoved`: the cursor position clamps at the window
    /// edge, so a drag that reaches the border would stop turning the camera.
    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
            self.input.add_mouse_delta(dx as f32, dy as f32);
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
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if let Some(key) = map_key(code) {
                        self.input.set_key(key, event.state == ElementState::Pressed);
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(b) = map_button(button) {
                    self.input.set_button(b, state == ElementState::Pressed);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.0,
                };
                self.input.add_scroll(lines);
            }
            // A key released while another window has focus never reports the
            // release, so without this the camera keeps flying after alt-tab.
            WindowEvent::Focused(false) => self.input.clear(),
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

/// winit key codes the engine reacts to. Anything else is ignored on purpose:
/// a key that nothing reads should do nothing, not something surprising.
fn map_key(code: KeyCode) -> Option<Key> {
    Some(match code {
        KeyCode::KeyW => Key::W,
        KeyCode::KeyA => Key::A,
        KeyCode::KeyS => Key::S,
        KeyCode::KeyD => Key::D,
        KeyCode::KeyQ => Key::Q,
        KeyCode::KeyE => Key::E,
        KeyCode::Space => Key::Space,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => Key::Shift,
        KeyCode::ControlLeft | KeyCode::ControlRight => Key::Control,
        KeyCode::Escape => Key::Escape,
        KeyCode::ArrowUp => Key::Up,
        KeyCode::ArrowDown => Key::Down,
        KeyCode::ArrowLeft => Key::Left,
        KeyCode::ArrowRight => Key::Right,
        KeyCode::Tab => Key::Tab,
        KeyCode::F1 => Key::F1,
        KeyCode::F2 => Key::F2,
        KeyCode::F3 => Key::F3,
        _ => return None,
    })
}

fn map_button(button: winit::event::MouseButton) -> Option<MouseButton> {
    Some(match button {
        winit::event::MouseButton::Left => MouseButton::Left,
        winit::event::MouseButton::Right => MouseButton::Right,
        winit::event::MouseButton::Middle => MouseButton::Middle,
        _ => return None,
    })
}
