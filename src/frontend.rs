// frontend: the native window (`nacs --view`). A thin egui shell.
// Everything on screen is derived elsewhere and pinned there: the outline
// tree, the drawable motion, the camera, the theme (the one colour home: this
// file builds no colour of its own — a slot, an alpha, nothing else; source-
// scan pinned in view_tests), the arrangement (`theme::Layout`, mutated only
// through `DockAction::apply`). If a fact on screen is wrong, the bug and the
// missing test are in those modules, not here — and since Phase 51 slice 1
// the shell itself is DRIVEN, headless, by `tests/shell_tests.rs` (behind
// the `shell-tests` feature): a thin test that asserts the shell SHOWS the
// fact, through `harness_app` below. The window may not say anything the
// CLI cannot: a project the engine rejects opens on its error. The contract
// for what this window IS lives in `DESIGN.md`.

use crate::gcode::OpEmission;
use crate::outline::{self, ProjectOutline};
use crate::parse::Part;
use crate::theme::{Center, DockAction, Document, Layer, Layout, SidebarTab, Theme};
use crate::check_result::CheckResult;
use crate::view::{self, Camera, FeatureEdges, OpPath};
use eframe::egui;

/// Open the window over an already-validated, already-emitted project.
/// Blocks until the window closes; errors (no display, no GPU adapter, a
/// missing platform library) come back as ONE LINE for main.rs's stderr —
/// the house error convention holds at this boundary even though the
/// windowing stack below it panics (winit's dynamic library loads panic
/// rather than erroring; that is not this project's convention, so the
/// panic is caught here and translated).
pub fn run(
    part: Part,
    check_result: CheckResult,
    project_path: &str,
    theme_arg: Option<String>,
    frame_native: Option<bool>,
) -> Result<(), String> {
    // The window's title is the path it was opened on, and that path is
    // also what it follows (DESIGN.md §1): one string, two readers.
    let title = project_path;
    name_the_gpu_driver();
    // The active theme: `--theme` wins, else the workspace file's `theme`
    // line, else the built-in palette. A chosen theme is loaded through
    // the policy (a meaning violation keeps the chrome and rejects the
    // viewport half, the reason named); a file that cannot be read is a
    // sentence and the built-in — and the path stays watched, so fixing
    // the file reloads it live.
    let workspace = crate::theme::load_full_workspace();
    let choice = theme_arg.or_else(|| workspace.theme.clone());
    let (theme, note, theme_path) = resolve_theme(&choice);
    let mut app = build(&part, check_result, project_path.to_string(), theme, choice, theme_path, note, workspace);
    // `--frame` is this launch's word over the workspace file's (Phase 57
    // slice 1, fork 1A); the file keeps what the window last switched to.
    if let Some(native) = frame_native {
        app.layout.frame_native = native;
    }
    // The last project, remembered at OPEN (Phase 50 slice 3), so `nacs
    // --view` alone reopens it even after a crash.
    if let Err(reason) = crate::theme::save_workspace(&app.workspace_snapshot()) {
        startup_say(format!("workspace not saved: {reason}"));
    }
    attempt_with_retry(app, title)
}

thread_local! {
    /// What the launch said before there was a window to say it in (the
    /// theme's loading): printed to stderr as ever and kept here for the
    /// Output log, since a GUI user on Windows never sees stderr.
    static STARTUP: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}
/// The same for what the window's creation says off this thread's reach
/// (the surface, the blur, the frame): only a real window writes here.
static WINDOW_SAID: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// A launch fact: to stderr, and kept for the Output log.
fn startup_say(text: String) {
    eprintln!("{text}");
    STARTUP.with(|q| q.borrow_mut().push(text));
}
fn window_say(text: String) {
    eprintln!("{text}");
    if let Ok(mut q) = WINDOW_SAID.lock() {
        q.push(text);
    }
}

/// The chosen theme, loaded: the file behind a name or a path, judged
/// through the policy; a file that cannot be read is a sentence and the
/// built-in. One home for the window and the harness, so a test opens on
/// exactly the theme a launch would.
fn resolve_theme(choice: &Option<String>) -> (Theme, Option<String>, Option<std::path::PathBuf>) {
    match choice {
        None => (Theme::default(), None, None),
        Some(c) => match crate::theme::theme_source(c) {
            Some(crate::theme::ThemeSource::File(path)) => {
                let (theme, note) = load_theme_file(&path, c);
                (theme, note, Some(path))
            }
            // Carried in the binary (Phase 51 slice 5): judged like a file,
            // watched by nothing — there is no file to change.
            Some(crate::theme::ThemeSource::Shipped(name, json)) => {
                startup_say(format!("Theme '{name}': the shipped copy (no file of that name in the themes folder)"));
                let (theme, note) = judge_theme_text(json, name, "the shipped copy");
                (theme, note, None)
            }
            None => {
                startup_say(format!("Could not load theme '{c}': no config dir to look in. Using the default theme."));
                (Theme::default(), Some(format!("Could not load theme '{c}' — no config dir")), None)
            }
        },
    }
}

/// The app for the shell tests (Phase 51 slice 1): the same `build` the
/// window runs, on a workspace the TEST hands in rather than the user's
/// file — so a test never reads `%APPDATA%\nacs\workspace` and the
/// arrangement it opens on is the one it asked for. No window, no GPU: the
/// harness (`egui_kittest`) runs `update` frame by frame and reads the
/// AccessKit tree. Nothing is saved here; the window's own saves happen
/// at open, at close and in two acts, which the tests redirect by
/// environment.
pub fn harness_app(part: &Part, check_result: CheckResult, project_path: &str, theme_arg: Option<String>, workspace: crate::theme::Workspace) -> ViewApp {
    let choice = theme_arg.or_else(|| workspace.theme.clone());
    let (theme, note, theme_path) = resolve_theme(&choice);
    build(part, check_result, project_path.to_string(), theme, choice, theme_path, note, workspace)
}

/// What a shell test may read back from the app beside the tree: the
/// MODEL behind a picture — so a test can say which of the two is wrong
/// when they disagree. Readers only; every write still goes through `act`.
impl ViewApp {
    pub fn layout(&self) -> &Layout {
        &self.layout
    }
    /// The Program's pane and its scroll area as last drawn (Phase 52 slice
    /// 3): `None` while the Program is not on screen.
    pub fn program_rects(&self) -> Option<(egui::Rect, egui::Rect)> {
        self.program_rects
    }
    /// The block cursor — the move within the cursor's operation, if one
    /// stands (Phase 52 slice 5).
    pub fn block_cursor(&self) -> Option<usize> {
        self.block_cursor
    }
    /// The emitted moves the window holds — the channel the Program lists.
    pub fn emitted_moves(&self) -> &[OpEmission] {
        &self.emitted_moves
    }
    /// The tool's pose as the viewport last drew it (tip, tool number).
    /// The op cursor — how many operations have run (Phase 52 slice 14).
    pub fn op_cursor(&self) -> usize {
        self.op_cursor
    }
    /// The Output log's lines as the dock prints them (Phase 54 slice 3).
    pub fn output_lines(&self) -> Vec<String> {
        self.output.events.iter().map(view::OutputLog::render).collect()
    }
    /// The runner's line as it stands (Phase 54 slices 2–3).
    pub fn runner_line(&self) -> &str {
        &self.runner_line
    }
    pub fn tool_pose_drawn(&self) -> Option<([f64; 3], usize)> {
        self.pose_drawn
    }
    /// The grips as the viewport last drew them: each key with its screen
    /// point (Phase 59 nudge 4), where `pick_handle` would pick it.
    pub fn grips_drawn(&self) -> &[(String, egui::Pos2)] {
        &self.grips_drawn
    }
    /// The repository's head as last read (Phase 52 slice 7): `None` until
    /// the worker answers.
    pub fn git_head(&self) -> Option<&Result<view::GitHead, String>> {
        self.git.as_ref()
    }
    /// What the stock surface in hand (or on the worker) was built through:
    /// the op cursor and the block (Phase 52 slice 6).
    pub fn sim_built_through(&self) -> (usize, Option<usize>) {
        (self.sim_cursor, self.sim_block)
    }
    pub fn palette_is_open(&self) -> bool {
        self.palette_open
    }
    pub fn help_is_open(&self) -> bool {
        self.help
    }
    pub fn selection(&self) -> view::Selection {
        self.selection
    }
    /// The features' names in edge order — `Selection::Feature(i)` is an
    /// index into these (ROADMAP item 3 slice 1).
    pub fn feature_names(&self) -> Vec<String> {
        self.edges.iter().map(|e| e.name.clone()).collect()
    }
    /// How many lines the program in hand has (ROADMAP item 3 slice 2): a
    /// reload that landed is a program that changed.
    pub fn program_lines(&self) -> usize {
        self.gcode.as_ref().map_or(0, |g| g.lines.len())
    }
    /// The Review's rows as derived (ROADMAP item 1, slice 3).
    pub fn review_rows(&self) -> &[view::ReviewRow] {
        &self.review_rows
    }
    /// The note under the last act — "Not written — …" — as it stands.
    pub fn edit_note(&self) -> Option<&str> {
        self.edit_note.as_deref()
    }
    /// The import's setup as the field holds it (ROADMAP item 1, slice 4).
    pub fn import_setup(&self) -> &view::ImportSetup {
        &self.import_setup
    }
    pub fn theme_note(&self) -> Option<&str> {
        self.theme_note.as_deref()
    }
    pub fn theme_choice(&self) -> Option<&str> {
        self.theme_choice.as_deref()
    }
    pub fn transient_note(&self) -> Option<&str> {
        self.transient.as_ref().map(|(s, _)| s.as_str())
    }
}

/// The keyboard, resolved: every action's default key, re-bound by the
/// workspace file's judged `key` lines. A key name nothing understands is
/// a sentence and the default; a default that cannot resolve (it can
/// always resolve — the inventory's keys are chosen from egui's names)
/// drops the binding rather than panicking.
/// The rail's width: the longest word it draws ("Diagnose", "Problems"),
/// some padding, and no more. It is a strip, and every point it takes is
/// one the viewport does not have.

fn build_keymap(overrides: &[(String, String)]) -> Vec<(&'static str, egui::Key)> {
    let resolved = view::resolve_keys(overrides);
    let mut map = Vec::new();
    for a in view::ACTIONS {
        let over = resolved.iter().find(|(id, _)| *id == a.id).map(|(_, k)| k.as_str());
        let name = over.unwrap_or(a.key);
        let key = egui::Key::from_name(name)
            .or_else(|| egui::Key::from_name(&name.to_uppercase()))
            .or_else(|| {
                if over.is_some() {
                    eprintln!(
                        "key `{name}` for `{}` is not a key this window knows — {}",
                        a.id,
                        if a.key.is_empty() {
                            "This command has no default shortcut".to_string()
                        } else {
                            format!("the default `{}` stands", a.key)
                        }
                    );
                }
                egui::Key::from_name(a.key).or_else(|| egui::Key::from_name(&a.key.to_uppercase()))
            });
        if let Some(k) = key {
            map.push((a.id, k));
        }
        // A second bare key (`Home` for the fit) rides beside the first;
        // a chord is the shell's own branch, not the keymap's.
        if let Some(more) = view::also(a.id).filter(|b| view::chord_key(b).is_none()) {
            if let Some(k) = egui::Key::from_name(more) {
                map.push((a.id, k));
            }
        }
    }
    map
}

/// Read, parse and judge one theme file, voicing every finding — the load
/// path and the hot-reload path speak with one voice: every finding on
/// stderr, and ONE summary line back for the window's own theme line (the
/// pane half of `--theme-check`'s voice). Any failure is a sentence and
/// the built-in palette. The first sentence, every time, is WHICH file
/// (Phase 24 slice 2, fork 4C): a bare name reads a copy in the user's
/// config dir, and a copy goes stale the day the repo's file changes —
/// so every verdict below names the file it is about. The window's theme
/// line stays one sentence (DESIGN §5.7) and points at `--theme-check`,
/// which says the same path.
fn load_theme_file(path: &std::path::Path, name: &str) -> (Theme, Option<String>) {
    startup_say(format!("Theme '{name}': reading {}", path.display()));
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Could not load theme '{name}': {} ({e}). Using the default theme.", path.display());
            // The PATH on the status line (Phase 51 slice 5, DESIGN §5.7): a
            // bare name resolves to the config folder, a reset empties it,
            // and "unreadable" said nothing about where to look.
            return (Theme::default(), Some(format!("Theme '{name}' not found at {} — the default worn", path.display())));
        }
    };
    judge_theme_text(&text, name, &path.display().to_string())
}

/// The judging half of a theme load, for a file's text and for a shipped
/// copy alike: parsed, the policy applied, every finding on stderr, one
/// summary line back for the status line. `where_` names the source.
fn judge_theme_text(text: &str, name: &str, where_: &str) -> (Theme, Option<String>) {
    let path_display = where_;
    match crate::theme::theme_from_json(text) {
        Ok(candidate) => {
            let (theme, findings) = crate::theme::apply_policy(candidate);
            for f in &findings {
                eprintln!("theme `{name}`: {}", f.message);
            }
            let meaning =
                findings.iter().filter(|f| f.grade == crate::theme::ThemeGrade::Meaning).count();
            let note = if meaning > 0 {
                eprintln!("Theme '{name}' ({path_display}): using the default palette for the viewport; the chrome half applies");
                Some(format!(
                    "Theme '{name}': {meaning} meaning violation(s) — using the default palette (nacs --theme-check {name})"
                ))
            } else {
                None
            };
            (theme, note)
        }
        Err(e) => {
            eprintln!("Could not load theme '{name}': {path_display} ({e}). Using the default theme.");
            (Theme::default(), Some(format!("Could not load theme '{name}' — not a theme file ({path_display})")))
        }
    }
}

fn attempt_with_retry(app: ViewApp, title: &str) -> Result<(), String> {
    match attempt(app, title) {
        Ok(()) => Ok(()),
        Err(first) => {
            // The WSLg wart, absorbed (found by HarveyShmarvey running slice
            // 1 on WSL): the session ADVERTISES Wayland, winit takes it at
            // its word, and the attempt then fails — while X11 through
            // XWayland works. The retry is the manual workaround
            // (`env -u WAYLAND_DISPLAY nacs --view …`) automated, and it must
            // be a RE-EXEC: winit rejects to create a second event loop in
            // the same process even when the first never existed
            // ("EventLoop can't be recreated" — measured, not assumed).
            // Window mode has written nothing to stdout, so replacing the
            // process loses nothing; the parse advisories print twice on the
            // retry path, which is the honest cost of a fresh start. The
            // marker env var makes one retry exactly one.
            let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
            let x11 = std::env::var_os("DISPLAY").is_some();
            let fresh = std::env::var_os("NACS_VIEW_NO_WAYLAND").is_none();
            #[cfg(unix)]
            if wayland && x11 && fresh {
                use std::os::unix::process::CommandExt;
                eprintln!("warning: {first} — retrying on X11 (the WSLg workaround, automated)");
                let exe = std::env::current_exe()
                    .map_err(|e| format!("cannot re-exec for the X11 retry: {e}"))?;
                let err = std::process::Command::new(exe)
                    .args(std::env::args_os().skip(1))
                    .env_remove("WAYLAND_DISPLAY")
                    .env("NACS_VIEW_NO_WAYLAND", "1")
                    .exec();
                return Err(format!("cannot re-exec for the X11 retry: {err}"));
            }
            #[cfg(not(unix))]
            let _ = (wayland, x11, fresh);
            Err(first)
        }
    }
}

/// The WSL software-rendering trap, absorbed — the Wayland re-exec's sibling,
/// found the same way (measured on HarveyShmarvey's box, 2026-08-11).
///
/// Mesa 25 stopped probing the `d3d12` driver on its own. On WSL that is
/// silent and expensive: `/dev/dxg` carries the GPU, but there is no DRM node
/// for the loader to match against, so it settles on `llvmpipe` and renders
/// every frame on the CPU without saying so — wgpu then reports two adapters
/// and BOTH are software. Nothing on screen names it, and a stock window
/// actively hides it: an empty frame measured FASTER under llvmpipe (13.84 ms
/// against 12.59 ms at 2544x1384) because software has nothing to rasterize.
/// The bill arrives with geometry — 20k line segments measured 220.73 ms
/// (4.5 fps) against 17.80 ms (56 fps) on the GPU, which is the difference
/// between a viewer and a slideshow.
///
/// So the driver is named here, and only where the shape of the machine says
/// llvmpipe is the sole other answer: WSL's GPU node present, no DRM node to
/// match. This is a default, not a policy — an explicit `GALLIUM_DRIVER` or
/// `MESA_LOADER_DRIVER_OVERRIDE` is left alone, because someone who names a
/// driver means it.
///
/// Naming a driver that cannot load needs no guard of its own: Mesa falls
/// back to llvmpipe and the window still opens (measured with a deliberately
/// bogus `GALLIUM_DRIVER`), so the worst case here is exactly the behaviour
/// this exists to replace.
fn name_the_gpu_driver() {
    let chosen = gpu_driver_default(
        std::env::var("GALLIUM_DRIVER").ok().as_deref(),
        std::env::var("MESA_LOADER_DRIVER_OVERRIDE").ok().as_deref(),
        std::path::Path::new("/dev/dxg").exists(),
        std::path::Path::new("/dev/dri").exists(),
    );
    if let Some(driver) = chosen {
        // Before `attempt`, and therefore before winit, wgpu, or the sim
        // worker exist: the environment is read by the GL loader when the
        // context is created, and `set_var` is only sound while this process
        // is still the single thread it is here.
        std::env::set_var("GALLIUM_DRIVER", driver);
    }
}

/// The decision `name_the_gpu_driver` acts on, pure so it can be pinned: the
/// driver to name, or `None` to leave Mesa's own choice standing. Public for
/// the tests — the shell is untested, but this is a DECISION, and the sliver
/// pin's rule (what the shell decides gets a test, what it merely shows does
/// not) applies to a driver exactly as it applies to a mesh.
///
/// A present-but-empty `GALLIUM_DRIVER` is not a choice — Mesa ignores it, so
/// this does too, rather than reading an accident as intent.
pub fn gpu_driver_default(
    gallium_driver: Option<&str>,
    loader_override: Option<&str>,
    wsl_gpu_node: bool,
    drm_node: bool,
) -> Option<&'static str> {
    let named = |v: Option<&str>| v.is_some_and(|s| !s.trim().is_empty());
    if named(gallium_driver) || named(loader_override) {
        return None;
    }
    // A DRM node means an ordinary Linux GPU stack the loader can match on
    // its own; no WSL GPU node means this is not the machine in question.
    if drm_node || !wsl_gpu_node {
        return None;
    }
    Some("d3d12")
}

// ---- the sim surface's own pipeline -----------------------------------------
//
// The one place this shell stops handing polylines to a painter. It must,
// because the surface it now draws is a general isosurface: there is no
// painter's order for one (the heightfield had one only because upward
// closure gave every XY column a single interval), so it needs a depth
// buffer, and a depth buffer needs a pipeline.
//
// The rule the file opens with still decides the split, it just lands one
// layer down: the GEOMETRY is derived and pinned in `vox::iso_surface`, the
// PROJECTION is derived and pinned as `view::Lens::clip_matrix`, and what is
// left here — a shader, a buffer, a draw call — is shell, and untested like
// the rest of the shell. Nothing below decides anything about the part.
//
// The mesh is uploaded ONCE and keyed on the field's own hash: orbiting sends
// sixteen floats, not a re-tessellated megabyte, which is what makes this
// slice a performance change as well as a capability one.

/// How many bits of depth the sim pipeline asks eframe for, and therefore
/// the format its own pipeline must declare. ONE constant, because egui keeps
/// its resolved format private and a mismatch is a startup validation failure
/// with no visible cause.
const SIM_DEPTH_BITS: u8 = 32;

/// Position + normal in, lambert grey out. The light and the ambient floor
/// are `view::SIM_LIGHT` and `view::SIM_AMBIENT` arriving as uniforms — the
/// color law has one home, and this shader is a consumer of it like
/// the shading law's only consumer now that the CPU one has retired.
const SIM_SHADER: &str = r#"
struct Uniforms {
    clip: mat4x4<f32>,
    // xyz = world light direction, w = ambient floor
    light: vec4<f32>,
    // rgb = base grey in sRGB 0-1, w = 1 when the target does the sRGB encode
    base: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>) -> VsOut {
    var out: VsOut;
    out.clip_pos = u.clip * vec4<f32>(pos, 1.0);
    out.normal = normal;
    return out;
}

// 0-1 linear from 0-1 sRGB gamma — egui's own conversion, restated because a
// render target that encodes sRGB on write needs LINEAR from the shader, and
// the base grey is authored in sRGB like every other color in this project.
fn linear_from_gamma(c: vec3<f32>) -> vec3<f32> {
    let cutoff = c < vec3<f32>(0.04045);
    let lower = c / vec3<f32>(12.92);
    let higher = pow((c + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4));
    return select(higher, lower, cutoff);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let lam = max(dot(n, normalize(u.light.xyz)), 0.0);
    let k = u.light.w + (1.0 - u.light.w) * lam;
    let shaded = u.base.rgb * k;
    let out_rgb = select(shaded, linear_from_gamma(shaded), u.base.w > 0.5);
    return vec4<f32>(out_rgb, 1.0);
}
"#;

/// What the pipeline needs that outlives a frame. Lives in egui's
/// `callback_resources`, created once at startup because the pipeline needs
/// the target format and that is known only there.
struct SimGpu {
    pipeline: eframe::wgpu::RenderPipeline,
    uniform: eframe::wgpu::Buffer,
    bind_group: eframe::wgpu::BindGroup,
    srgb: bool,
    /// The uploaded mesh, and the field id it was extracted from. `None`
    /// until the sim is first switched on.
    mesh: Option<SimMeshGpu>,
}

struct SimMeshGpu {
    verts: eframe::wgpu::Buffer,
    indices: eframe::wgpu::Buffer,
    n_indices: u32,
    id: u64,
}

/// Build the pipeline and hand it to egui's resource map. Called once, from
/// the eframe creation closure, which is the only place the render state — and
/// therefore the target format — is in reach.
fn init_sim_gpu(rs: &eframe::egui_wgpu::RenderState) {
    use eframe::wgpu;
    let device = &rs.device;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("nacs sim surface"),
        source: wgpu::ShaderSource::Wgsl(SIM_SHADER.into()),
    });
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("nacs sim bgl"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("nacs sim uniforms"),
        size: 96, // mat4 + two vec4
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("nacs sim bind group"),
        layout: &bgl,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("nacs sim layout"),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("nacs sim pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: 24,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 12,
                        shader_location: 1,
                    },
                ],
            })],
        },
        primitive: wgpu::PrimitiveState {
            // NO back-face culling, deliberately. The extraction's winding is
            // pinned outward, so culling would work — but a machinist orbiting
            // under the table should see the inside of the shell rather than
            // an object that vanishes, and the depth test already orders it.
            cull_mode: None,
            ..Default::default()
        },
        // The format is derived from the SAME constant handed to
        // `NativeOptions::depth_buffer`, because egui keeps its own copy
        // private and two independently-chosen depth formats is a pipeline
        // that fails to validate at startup for a reason nobody can see.
        depth_stencil: eframe::egui_wgpu::depth_format_from_bits(SIM_DEPTH_BITS, 0).map(|format| {
            wgpu::DepthStencilState {
                format,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: rs.target_format,
                blend: None, // the stock is opaque; the depth test does the ordering
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    rs.renderer.write().callback_resources.insert(SimGpu {
        pipeline,
        uniform,
        bind_group,
        srgb: rs.target_format.is_srgb(),
        mesh: None,
    });
}

/// One frame's worth of sim drawing: the camera as sixteen floats, and the
/// mesh it should be drawn against.
struct SimCallback {
    clip: [f32; 16],
    iso: std::sync::Arc<view::StockSurface>,
    /// The surface's base colour — the theme's `sim.base` slot, carried in
    /// because the callback outlives the frame and cannot reach the app.
    base: [u8; 3],
}

impl eframe::egui_wgpu::CallbackTrait for SimCallback {
    fn prepare(
        &self,
        device: &eframe::wgpu::Device,
        queue: &eframe::wgpu::Queue,
        _screen: &eframe::egui_wgpu::ScreenDescriptor,
        _encoder: &mut eframe::wgpu::CommandEncoder,
        resources: &mut eframe::egui_wgpu::CallbackResources,
    ) -> Vec<eframe::wgpu::CommandBuffer> {
        use eframe::wgpu;
        let Some(gpu) = resources.get_mut::<SimGpu>() else {
            return Vec::new();
        };

        // The upload happens when the FIELD changed and not otherwise. This
        // is the whole performance story of the slice: an orbit re-uploads
        // nothing, because the surface is a function of the field and the
        // field's hash says so.
        let stale = gpu.mesh.as_ref().is_none_or(|m| m.id != self.iso.id);
        if stale {
            let vbytes: &[u8] = bytemuck_cast(&self.iso.verts);
            let ibytes: &[u8] = bytemuck_cast(&self.iso.tris);
            let verts = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nacs sim verts"),
                size: vbytes.len() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let indices = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nacs sim indices"),
                size: ibytes.len() as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&verts, 0, vbytes);
            queue.write_buffer(&indices, 0, ibytes);
            gpu.mesh = Some(SimMeshGpu {
                verts,
                indices,
                n_indices: self.iso.tris.len() as u32,
                id: self.iso.id,
            });
        }

        let l = view::SIM_LIGHT;
        let b = self.base;
        let mut u = [0f32; 24];
        u[..16].copy_from_slice(&self.clip);
        u[16..20].copy_from_slice(&[
            l[0] as f32,
            l[1] as f32,
            l[2] as f32,
            view::SIM_AMBIENT as f32,
        ]);
        u[20..24].copy_from_slice(&[
            f32::from(b[0]) / 255.0,
            f32::from(b[1]) / 255.0,
            f32::from(b[2]) / 255.0,
            if gpu.srgb { 1.0 } else { 0.0 },
        ]);
        queue.write_buffer(&gpu.uniform, 0, bytemuck_cast(&u));
        Vec::new()
    }

    fn paint(
        &self,
        _info: eframe::egui::PaintCallbackInfo,
        pass: &mut eframe::wgpu::RenderPass<'static>,
        resources: &eframe::egui_wgpu::CallbackResources,
    ) {
        let Some(gpu) = resources.get::<SimGpu>() else { return };
        let Some(mesh) = gpu.mesh.as_ref() else { return };
        pass.set_pipeline(&gpu.pipeline);
        pass.set_bind_group(0, &gpu.bind_group, &[]);
        pass.set_vertex_buffer(0, mesh.verts.slice(..));
        pass.set_index_buffer(mesh.indices.slice(..), eframe::wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.n_indices, 0, 0..1);
    }
}

/// Plain-old-data to bytes, without a crate for it. Every caller below hands
/// this a slice of `f32` arrays or `u32`, which are `Copy`, contain no
/// padding at these sizes, and have no invalid bit patterns — the three
/// conditions that make the reinterpretation sound.
fn bytemuck_cast<T: Copy>(v: &[T]) -> &[u8] {
    // SAFETY: `T` here is only ever `[f32; 6]`, `f32` or `u32` (see the call
    // sites); each is `Copy`, densely packed, and valid for every bit
    // pattern, so a read-only view of its bytes cannot observe uninitialised
    // memory or an invalid value. The lifetime is tied to `v`.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

/// Everything the window derives from a loaded project and its check
/// result — ONE list, read at open and again at every reload (DESIGN.md
/// §1, the window follows the file), so a reload cannot forget a field
/// the open remembered.
struct Derived {
    rows: Vec<view::FindingRow>,
    annotations: Vec<view::Annotation>,
    /// The check's account (ROADMAP item 2, slice 7): what each family
    /// judged, found and could not judge — the Problems pane's footing.
    account: Vec<crate::check_result::FamilyAccount>,
    /// A move-less copy of the check result stays aboard: a theme reload
    /// rebakes the rows and annotations with the new slots, and
    /// `finding_rows` reads the error and the findings, never the moves.
    check_result_kept: CheckResult,
    /// A rejected program has no emitted moves: no paths, and the scene is
    /// the stock and its edges — the diagnostics stance.
    emitted_moves: Vec<OpEmission>,
    paths: Vec<OpPath>,
    edges: Vec<FeatureEdges>,
    /// Wires join the fit: a declared-but-unrun dome has no toolpath, and
    /// cropping it would hide exactly the mistake the outline's "declared,
    /// never run" tail exists to show. One box, shared by the first-frame
    /// fit and the minimap's plan fit — the two must frame the same scene.
    fit_bb: [f64; 6],
    outline: ProjectOutline,
    /// Derived once, like every other view model this shell only paints.
    /// The emitter runs a second time rather than the text being smuggled
    /// out of the check result: `program_with_moves` is pure, and a cached
    /// string is a second copy to keep honest.
    gcode: Result<view::GcodeView, String>,
    /// The clock (Phase 50 slice 8): `cut_time::time_report`, the CLI's own.
    clock: Result<crate::cut_time::TimeReport, String>,
    fixture: Vec<Vec<[f64; 3]>>,
}

/// The suppressed operations, read off the project's source files — the
/// two part files, which are where `run` lines live.
fn scan_suppressed(part: &Part) -> Vec<view::SuppressedOp> {
    let texts: Vec<String> = part.sources.iter().take(2).map(|p| std::fs::read_to_string(p).unwrap_or_default()).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    view::suppressed_ops(&refs)
}

fn derive(part: &Part, check_result: CheckResult, theme: &Theme) -> Derived {
    let (rows, annotations) = view::finding_rows(&check_result, part.stock.z, theme);
    let account = crate::check_result::account(part, &check_result);
    let check_result_kept = CheckResult {
        refusal: check_result.refusal.clone(),
        findings: check_result.findings.clone(),
        emitted_moves: None,
    };
    let emitted_moves: Vec<OpEmission> = check_result.emitted_moves.unwrap_or_default();
    let paths = view::op_paths(part, &emitted_moves);
    let edges = view::feature_edges(part);
    let fit_bb = view::edges_aabb(&edges, view::scene_aabb(&part.stock, &paths));
    Derived {
        rows,
        annotations,
        account,
        check_result_kept,
        emitted_moves,
        paths,
        edges,
        fit_bb,
        outline: outline::outline(part),
        gcode: view::gcode_view(part),
        clock: crate::cut_time::time_report(part),
        fixture: view::fixture_boundaries(part),
    }
}

/// The view models, derived fresh — cheap and pure, so a retry rebuilds
/// rather than cloning a consumed app.
fn build(
    part: &Part,
    check_result: CheckResult,
    project_path: String,
    theme: Theme,
    theme_choice: Option<String>,
    theme_path: Option<std::path::PathBuf>,
    theme_note: Option<String>,
    workspace: crate::theme::Workspace,
) -> ViewApp {
    let keymap = build_keymap(&workspace.keys);
    let project_mtime = view::newest_mtime(&part.sources);
    // The report the sidecar was told to write beside the project
    // (`-o <project dir>/`): the directory itself, or a bare file's.
    let project = std::path::Path::new(&project_path);
    // A `.part` is a file in the project's folder; anything else IS the
    // folder, whether or not it exists yet (a fresh folder's import makes it).
    let report_path = if project.extension().is_some_and(|e| e == "part") { project.parent().map(|d| d.to_path_buf()).unwrap_or_default() } else { project.to_path_buf() }.join("report.json");
    let theme_mtime = theme_path
        .as_deref()
        .and_then(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    let d = derive(part, check_result, &theme);
    let k = part.operations.len();
    let removal = (k, view::removal_boundaries(part, k));
    // The open-on-error stance: a rejected project opens with Problems
    // showing. Still one rule, at open, for one condition — the decision
    // itself is `Layout::at_open`, std-only, where a pin reaches it.
    let layout = Layout::at_open(d.check_result_kept.refusal.is_some(), workspace.layout);
    // Read before `theme` moves into the struct (Phase 51 slice 5).
    let glass_asked_at_launch = theme.background_appearance != crate::theme::BackgroundAppearance::Opaque;
    ViewApp {
        outline: d.outline,
        paths: d.paths,
        edges: d.edges,
        fit_bb: d.fit_bb,
        rows: d.rows,
        account: d.account,
        // Derived once, like every other view model this shell only
        // paints. The emitter runs a second time rather than the text
        // being smuggled out of the check result: `program_with_moves` is pure,
        // and a cached string is a second copy to keep honest.
        gcode: d.gcode,
        clock: d.clock,
        part: part.clone(),
        cam: Camera::default_orbit(),
        fitted: false,
        focus_pending: false,
        tween: None,
        grip: None,
        vp_size: (0.0, 0.0),
        map_gesture: None,
        sim: None,
        sim_rx: None,
        annotations: d.annotations,
        emitted_moves: std::sync::Arc::new(d.emitted_moves),
        op_cursor: k,
        cursor_after_write: None,
        select_after_write: None,
        block_cursor: None,
        scroll_program_to_block: false,
        scroll_program_to_op: None,
        scroll_sidebar_to_op: None,
        zoom: workspace.zoom.unwrap_or(1.0),
        zoom_applied: false,
        glass_granted: std::sync::Arc::new(std::sync::Mutex::new(None)),
        blur: view::Blur::NotAsked,
        frame_dwm: view::FrameDwm::NotAsked,
        pending_viewport: Vec::new(),
        maximized: false,
        removal,
        cells: None,
        immersion: None,
        immersion_blocks: None,
        cells_rx: None,
        sim_cursor: k,
        sim_block: None,
        selection: view::Selection::None,
        fixture: d.fixture,
        help: false,
        theme,
        theme_choice,
        theme_path,
        theme_mtime,
        theme_note,
        project_path,
        project_mtime,
        project_note: None,
        project_reload: false,
        reload_rx: None,
        edits: view::EditHistory::default(),
        edit_note: None,
        runner_refusal: None,
        runner_history: view::LineHistory::default(),
        transient: None,
        program_copy_pending: false,
        project_switch: false,
        quick_shape: Default::default(),
        hover_feature: None,
        import_busy: None,
        import_rx: None,
        review_dups: Vec::new(),
        suppressed: scan_suppressed(part),
        field_edit: Vec::new(),
        pattern_table: None,
        pattern_rx: None,
        walk: None,
        walk_rx: None,
        bench: std::env::var("NACS_FRAME_BENCH").ok().and_then(|v| v.parse().ok()).map(FrameBench::new),
        new_form: None,
        report_path,
        report_mtime: None,
        report: None,
        review_rows: Vec::new(),
        review_drafts: std::collections::BTreeMap::new(),
        stock_mtime: None,
        stock_text: None,
        stock_proposal: None,
        stock_declined: None,
        proposal_edges: None,
        check_result: d.check_result_kept,
        // The window opens the way it was closed — the workspace file,
        // or the default when there is none — and in
        // the mode it was closed in, unless the project was rejected,
        // which opens on diagnose.
        layout,
        raw_keys: workspace.keys,
        shell: workspace.shell,
        keymap,
        palette_open: false,
        palette_query: String::new(),
        palette_sel: 0,
        glass_asked_at_launch,
        overlay_opened_this_frame: false,
        layers_list_shown: false,
        layers_hover_since: None,
        program_rects: None,
        program_controls_w: 0.0,
        center_total: 0.0,
        split_epoch: 0,
        sidebar_close_w: 24.0,
        viewport_strip_w: 0.0,
        output: view::OutputLog::default(),
        opened_at: std::time::Instant::now(),
        opened_unix: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        output_copy_pending: false,
        notes_seen: Vec::new(),
        runner_line: String::new(),
        runner_rx: None,
        runner_busy: None,
        runner_child: None,
        runner_killed: false,
        runner_focus_pending: false,
        target_path_draft: None,
        import_setup: view::ImportSetup::default(),
        import_last: None,
        pose_drawn: None,
        grips_drawn: Vec::new(),
        git: None,
        git_rx: None,
        git_read_at: None,
        dialog_rx: None,
        panel_epoch: 0,
    }
}

/// The adapter selection (egui-wgpu's `native_adapter_selector`; Phase 25
/// slice 1's spike, kept): every adapter's surface is asked what
/// composite-alpha modes it offers, the list is said once on stderr, and
/// an adapter whose surface can be see-through is preferred when the
/// theme asked for glass (`background.appearance` other than opaque —
/// slice 2; the spike asked on Windows unconditionally, and the user's
/// launch answered: the Vulkan surface offers PreMultiplied). The rule is
/// `view::pick_adapter`, pinned on plain data; this only reads wgpu, and
/// writes what the surface said where the status line can read it.
fn select_adapter(
    adapters: &[eframe::wgpu::Adapter],
    surface: Option<&eframe::wgpu::Surface<'_>>,
    want_glass: bool,
    granted: &std::sync::Mutex<Option<bool>>,
) -> Result<eframe::wgpu::Adapter, String> {
    let offers: Vec<view::SurfaceOffer> = adapters
        .iter()
        .map(|a| {
            let info = a.get_info();
            let alpha_modes = surface
                .map(|s| s.get_capabilities(a).alpha_modes.iter().map(|m| format!("{m:?}")).collect())
                .unwrap_or_default();
            view::SurfaceOffer {
                backend: info.backend.to_string(),
                name: info.name,
                kind: match info.device_type {
                    eframe::wgpu::DeviceType::DiscreteGpu => view::DeviceKind::Discrete,
                    eframe::wgpu::DeviceType::IntegratedGpu => view::DeviceKind::Integrated,
                    eframe::wgpu::DeviceType::VirtualGpu => view::DeviceKind::Virtual,
                    eframe::wgpu::DeviceType::Cpu => view::DeviceKind::Cpu,
                    eframe::wgpu::DeviceType::Other => view::DeviceKind::Other,
                },
                alpha_modes,
            }
        })
        .collect();
    let chosen = view::pick_adapter(&offers, want_glass);
    window_say(view::surface_sentence(&offers, chosen, want_glass));
    if let Ok(mut g) = granted.lock() {
        *g = chosen.map(|i| offers[i].transparent());
    }
    chosen
        .map(|i| adapters[i].clone())
        .ok_or_else(|| String::from("no graphics adapter offered a surface for the window"))
}

/// The blur (Phase 25 slice 3, fork 5): Windows 11's system backdrop
/// behind the window — acrylic (`DWMSBT_TRANSIENTWINDOW`), one
/// `DwmSetWindowAttribute` on the HWND winit made, the same call Zed
/// makes for its blur themes. It shows through wherever the swapchain's
/// alpha lets it, which is why it is asked for only after the surface
/// granted the glass. Windows 11 22H2 knows the attribute; anything older
/// answers with an HRESULT, which the sentence carries. Set ONCE: a
/// screen capture of the glass shows an opaque box at the window's
/// creation size, which is the HWND's never-painted GDI redirection
/// surface composed under the glass by DWM's capture path, not the
/// backdrop — setting this again does nothing to it, and painting the
/// surface takes the acrylic off the screen (Phase 54 slice 4's
/// erratum). The cure is `WS_EX_NOREDIRECTIONBITMAP` at creation, which
/// eframe does not reach.
#[cfg(windows)]
fn apply_blur(window: &impl eframe::wgpu::rwh::HasWindowHandle) -> view::Blur {
    use eframe::wgpu::rwh::RawWindowHandle;
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWM_SYSTEMBACKDROP_TYPE};
    let hwnd = match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get() as *mut core::ffi::c_void,
        Ok(_) => return view::Blur::Refused(String::from("the window handle is not a Win32 HWND")),
        Err(e) => return view::Blur::Refused(format!("no window handle: {e}")),
    };
    let backdrop: DWM_SYSTEMBACKDROP_TYPE = DWMSBT_TRANSIENTWINDOW;
    // SAFETY: `hwnd` is the live window winit created for this app and
    // `backdrop` is the 4-byte value the attribute documents; the call
    // reads it and returns.
    let hr = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE as u32,
            (&backdrop as *const DWM_SYSTEMBACKDROP_TYPE).cast::<core::ffi::c_void>(),
            std::mem::size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
        )
    };
    if hr == 0 {
        view::Blur::Applied
    } else {
        view::Blur::Refused(format!("DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE) returned HRESULT {hr:#010x} — Windows 11 22H2 or newer knows the attribute"))
    }
}

/// The window's own frame, finished by the platform (Phase 57 slice 1,
/// fork 0A): on Windows 11 the corners rounded (`DWMWCP_ROUND`) and the
/// frame's one-pixel line the theme's `border` colour — two
/// `DwmSetWindowAttribute` calls on the HWND winit made without a caption,
/// the blur's pattern, through the ledger's crate and no other. Zed asks
/// for the same two. A refusal is said once, as the blur's is.
#[cfg(windows)]
fn apply_frame(window: &impl eframe::wgpu::rwh::HasWindowHandle, border: [u8; 3]) -> view::FrameDwm {
    use eframe::wgpu::rwh::RawWindowHandle;
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DWM_WINDOW_CORNER_PREFERENCE};
    let hwnd = match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get() as *mut core::ffi::c_void,
        Ok(_) => return view::FrameDwm::Refused(String::from("the window handle is not a Win32 HWND")),
        Err(e) => return view::FrameDwm::Refused(format!("no window handle: {e}")),
    };
    let corner: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
    // A COLORREF is 0x00BBGGRR.
    let colour: u32 = (border[2] as u32) << 16 | (border[1] as u32) << 8 | border[0] as u32;
    // SAFETY: `hwnd` is the live window winit created for this app; each
    // value is the 4-byte word its attribute documents; the calls read it
    // and return.
    let (hr_corner, hr_border) = unsafe {
        (
            DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE as u32, (&corner as *const DWM_WINDOW_CORNER_PREFERENCE).cast::<core::ffi::c_void>(), std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32),
            DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR as u32, (&colour as *const u32).cast::<core::ffi::c_void>(), std::mem::size_of::<u32>() as u32),
        )
    };
    if hr_corner == 0 && hr_border == 0 {
        view::FrameDwm::Applied { border }
    } else {
        view::FrameDwm::Refused(format!("DwmSetWindowAttribute returned HRESULT {hr_corner:#010x} for the corners and {hr_border:#010x} for the border — Windows 11 knows both attributes"))
    }
}

#[cfg(not(windows))]
fn apply_frame(_window: &impl eframe::wgpu::rwh::HasWindowHandle, _border: [u8; 3]) -> view::FrameDwm {
    view::FrameDwm::NotOnThisPlatform
}

/// Linux has no portable blur (KDE and Hyprland take a hint, GNOME none —
/// named out); the glass shows the desk as it is, and the line says so.
#[cfg(not(windows))]
fn apply_blur(_window: &impl eframe::wgpu::rwh::HasWindowHandle) -> view::Blur {
    view::Blur::NotOnThisPlatform
}

/// The native open-file dialog for a STEP (DESIGN.md §5.2, Phase 52 slice
/// 9): `GetOpenFileNameW`, comdlg32's own, the second Win32 call this
/// program makes and the same shape as the blur's — one call, no crate
/// (`rfd` declined). BLOCKS until the dialog closes, so the shell runs it
/// on a worker thread and reads the answer back; `None` is a cancel.
#[cfg(windows)]
fn open_step_dialog(initial_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    use windows_sys::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW};
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    // A filter is pairs of NUL-terminated strings, ended by a second NUL.
    let filter: Vec<u16> = "STEP files (*.step, *.stp)\0*.step;*.stp\0All files\0*.*\0\0".encode_utf16().collect();
    let title = wide("Import a STEP file as the target");
    let dir = wide(&initial_dir.to_string_lossy());
    let mut file = vec![0u16; 32768];
    // SAFETY: the struct is plain data the call reads; every pointer in it
    // lives on this stack frame for the whole blocking call, and the
    // buffer's length is handed over beside it.
    let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.nFilterIndex = 1;
    ofn.lpstrFile = file.as_mut_ptr();
    ofn.nMaxFile = file.len() as u32;
    ofn.lpstrInitialDir = dir.as_ptr();
    ofn.lpstrTitle = title.as_ptr();
    ofn.Flags = OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    let ok = unsafe { GetOpenFileNameW(&mut ofn) };
    if ok == 0 {
        return None;
    }
    let len = file.iter().position(|&c| c == 0).unwrap_or(0);
    Some(std::path::PathBuf::from(String::from_utf16_lossy(&file[..len])))
}

/// No native dialog is wired on this platform; the path field and the
/// drop stand (the button is not drawn).
#[cfg(not(windows))]
fn open_step_dialog(_initial_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    None
}

/// Whether *Browse…* has a dialog to open on this platform.
const HAS_FILE_DIALOG: bool = cfg!(windows);

/// One `run_native` attempt, with winit's panics caught at the boundary and
/// translated to the convention: a missing platform library is an
/// environment error with the package named, not a backtrace.
fn attempt(app: ViewApp, title: &str) -> Result<(), String> {
    let want_glass = app.theme.background_appearance != crate::theme::BackgroundAppearance::Opaque;
    let granted = app.glass_granted.clone();
    // The window's own frame (Phase 57 slice 1, fork 0A): no decorations
    // unless the workspace word or `--frame native` asked for the system's;
    // the band is the title bar then, and `apply_frame` asks the platform
    // for the corners and the border once the window stands.
    let own_frame = !app.layout.frame_native;
    let border = app.theme.border;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title.to_string())
            .with_decorations(!own_frame)
            .with_inner_size(egui::Vec2::new(1200.0, 800.0))
            // The glass (Phase 25): a theme whose `background.appearance`
            // is not opaque asks for a see-through window; an opaque theme
            // gets the window it always got. Whether the swapchain grants
            // it is what `select_adapter` reads and says; on a surface that
            // offers only Opaque the glass is worn over black.
            .with_transparent(want_glass),
        // The isosurface has no painter's order — the whole reason this slice
        // exists — so the window carries a depth buffer.
        depth_buffer: SIM_DEPTH_BITS,
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            // NO vsync (Phase 59 slice 3, fork 2A). The glass needs the
            // Vulkan surface — the one that offers PreMultiplied
            // (`select_adapter`, since Phase 25) — and under egui-wgpu's
            // default AutoVsync that surface presented one frame every
            // 31 ms on the user's 164 Hz desk: 32 fps under every drag,
            // the "jumpy" of the Phase 58 walk, whatever theme, frame or
            // layer. AutoNoVsync — Mailbox where the surface offers it,
            // Immediate otherwise — gave 4.7 ms on the same surface, the
            // frame's own cost; the bench (`NACS_FRAME_BENCH`) is how it is
            // re-read, and the slice entry carries the numbers. egui
            // paints only when something changed or asked, so an idle
            // window still costs nothing, and a drag costs one frame per
            // coalesced pointer event, as before. WHY the FIFO surface
            // presents at a third of the display's rate is not claimed.
            surface: eframe::egui_wgpu::SurfaceConfig { present_mode: eframe::wgpu::PresentMode::AutoNoVsync, ..eframe::egui_wgpu::SurfaceConfig::HIGH_THROUGHPUT },
            wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(eframe::egui_wgpu::WgpuSetupCreateNew {
                native_adapter_selector: Some(std::sync::Arc::new(move |adapters, surface| {
                    select_adapter(adapters, surface, want_glass, &granted)
                })),
                ..eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let title_owned = title.to_string();
    // The default hook would print the panic (and any backtrace) BEFORE the
    // catch below can translate it. Silence exactly the environment-shaped
    // panic — a dynamic library that "could not be loaded" is a missing
    // runtime dependency, not an internal invariant — and delegate anything
    // else to the previous hook, so a real bug inside a frame keeps its loud
    // backtrace (the panic convention is for invariants, and those should
    // stay loud).
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let env_shaped = info
            .payload()
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| info.payload().downcast_ref::<&str>().copied())
            .is_some_and(|m| m.contains("could not be loaded"));
        if !env_shaped {
            previous_hook(info);
        }
    }));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        eframe::run_native(
            &title_owned,
            options,
            Box::new(move |cc| {
                // The only place the render state — and therefore the target
                // format the pipeline must declare — is in reach.
                if let Some(rs) = cc.wgpu_render_state.as_ref() {
                    init_sim_gpu(rs);
                }
                // And the only place the window itself is in reach before
                // the first frame: a `blurred` theme asks the platform for
                // its backdrop here (slice 3), when the surface granted the
                // glass there is to blur.
                let mut app = app;
                let granted_glass = app.glass_granted.lock().ok().and_then(|g| *g) == Some(true);
                if app.theme.background_appearance == crate::theme::BackgroundAppearance::Blurred && granted_glass {
                    app.blur = apply_blur(cc);
                    window_say(view::blur_sentence(&app.blur));
                }
                if own_frame {
                    app.frame_dwm = apply_frame(cc, border);
                }
                window_say(view::frame_sentence(!own_frame, &app.frame_dwm));
                Ok(Box::new(app))
            }),
        )
    }));
    let _ = std::panic::take_hook(); // restore default behavior past this boundary
    match result {
        Ok(r) => r.map_err(|e| format!("cannot open the view window: {e}")),
        Err(payload) => {
            let msg = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("the windowing stack panicked");
            if msg.contains("xkbcommon") {
                Err(format!(
                    "cannot open the view window: {msg} — the X11 keyboard library is a runtime dependency; on Debian/Ubuntu (and WSL): sudo apt install libxkbcommon-x11-0"
                ))
            } else {
                Err(format!("cannot open the view window: {msg}"))
            }
        }
    }
}

/// What a field edit rewrites: the statement whose line carries the key —
/// the view model's own name for it (Phase 21 slice 1).
type FieldTarget = view::Statement;

/// The seventh write's draft (DESIGN.md §1): the creation form's unsent
/// text — a variant index into `template::FEATURE_SPECS`, the name, one
/// buffer per spec key, the points buffer for a block variant. Never a
/// remembered file value: cleared on create and on Esc, and the buffers
/// reset when the variant changes (a stale `height` on a variant that
/// takes none would ride into the statement as the parser's problem —
/// loudly, but pointlessly).
struct NewForm {
    head: NewHead,
    /// The feature's name, or the tool's number slot (`T3`).
    name: String,
    values: Vec<String>,
    points: String,
}

/// Which statement the form composes (DESIGN.md §5.4): a feature by its
/// index into `FEATURE_SPECS`, or a tool by its index into `TOOL_SPECS`.
#[derive(Clone, PartialEq, Eq)]
enum NewHead {
    Feature(usize),
    Tool(usize),
    /// The ninth write: a run line on a feature, from its template
    /// skeletons — `skeletons` are `template::op_lines`' texts, `pick`
    /// the one the form fills.
    Run { feature: String, skeletons: Vec<String>, pick: usize },
    /// A main.part from nothing (Phase 50 slice 3): the stock kind by its
    /// index into `template::PROJECT_STOCK_KINDS`.
    Project(usize),
}

/// What a Create made, for the reload to select (ROADMAP item 3 slice 1):
/// a feature by name, an operation by the index it landed at, a tool by
/// number — the three the reload can find again.
enum Made {
    Feature(String),
    Operation(usize),
    Tool(usize),
}

/// A row's third door (DESIGN.md §5.2, §6, Phase 50 slice 9): a right-click
/// SELECTS the row (`pick`) and its menu offers `view::context_actions`;
/// an entry picked fires through the one dispatcher (`act`, applied by the
/// pane after the selection lands); a double-click focuses. The shell
/// draws; the list is the view model's.
fn row_menu(resp: &egui::Response, sel: view::Selection, pick: &mut Option<view::Selection>, act: &mut Option<&'static str>) {
    if resp.secondary_clicked() {
        *pick = Some(sel);
    }
    if resp.double_clicked() {
        *pick = Some(sel);
        *act = Some("view.focus");
    }
    resp.context_menu(|ui| {
        for (id, label) in view::context_actions(sel) {
            if ui.button(label).clicked() {
                *pick = Some(sel);
                *act = Some(id);
                ui.close();
            }
        }
    });
}

/// A new operation's blanks answered where the project already knows
/// them (DESIGN.md §5.4, Phase 50 slice 5; tenet 2): `T?` the only
/// declared tool when there is one, `stepover=?` half that tool's
/// diameter — shown, editable, committed by Create and never before.
fn prefill_run(part: &Part, form: &mut NewForm) {
    let NewHead::Run { ref skeletons, pick, .. } = form.head else { return };
    let blanks = skeletons.get(pick).map(|s| crate::template::skeleton_blanks(s)).unwrap_or_default();
    let lone = view::reference_options(part, "tool").filter(|o| o.len() == 1).map(|o| o[0].clone());
    for (i, (key, _)) in blanks.iter().enumerate() {
        if key == "tool" && form.values[i].is_empty() {
            if let Some(t) = &lone {
                form.values[i] = t.clone();
            }
        }
    }
    let tool = blanks.iter().position(|(k, _)| k == "tool").map(|i| form.values[i].clone()).unwrap_or_default();
    for (i, (key, _)) in blanks.iter().enumerate() {
        if key == "stepover" && form.values[i].is_empty() {
            if let Some(s) = crate::template::suggested_stepover(part, &tool) {
                form.values[i] = s;
            }
        }
    }
}

impl NewForm {
    fn for_variant(variant: usize) -> Self {
        NewForm {
            head: NewHead::Feature(variant),
            name: String::new(),
            values: vec![String::new(); crate::template::FEATURE_SPECS[variant].keys.len()],
            points: String::new(),
        }
    }

    /// The tool head: the number slot prefilled with the lowest undeclared
    /// `T` — tenet 2's smart default, derived from the declared tools.
    fn for_tool(kind: usize, number: usize) -> Self {
        let n = crate::template::tool_keys(&crate::template::TOOL_SPECS[kind]).len();
        NewForm { head: NewHead::Tool(kind), name: format!("T{number}"), values: vec![String::new(); n], points: String::new() }
    }

    /// The project head: the stock's keys and the header's six, the
    /// defaults prefilled and visible (tenet 2: shown, never silent).
    fn for_project(kind: usize) -> Self {
        let keys = crate::template::project_keys(crate::template::PROJECT_STOCK_KINDS[kind]).unwrap_or_default();
        NewForm { head: NewHead::Project(kind), name: String::new(), values: keys.into_iter().map(|(_, _, d)| d).collect(), points: String::new() }
    }

    /// The run head: the feature's skeletons, the picked one's blanks as
    /// the fields (the skeleton is the form).
    fn for_run(feature: String, skeletons: Vec<String>, pick: usize) -> Self {
        let n = skeletons.get(pick).map(|s| crate::template::skeleton_blanks(s).len()).unwrap_or(0);
        NewForm { head: NewHead::Run { feature, skeletons, pick }, name: String::new(), values: vec![String::new(); n], points: String::new() }
    }
}

/// The window's state. `pub` since Phase 51 slice 1 so the shell tests can
/// name it as the harness's state type; every field stays private.
pub struct ViewApp {
    /// `NACS_FRAME_BENCH=<frames>`: repaint continuously, time each frame's
    /// sections, print the averages to stderr and close. A measuring
    /// instrument for "it's a slideshow", with nothing to say otherwise.
    bench: Option<FrameBench>,
    part: Part,
    outline: ProjectOutline,
    paths: Vec<OpPath>,
    /// The declared features' geometry — derived once in
    /// `run()`, like everything else this shell only paints.
    edges: Vec<FeatureEdges>,
    /// The whole scene's box (stock + paths + edges), derived once — the
    /// first-frame fit and the minimap's plan fit read the same one.
    fit_bb: [f64; 6],
    cam: Camera,
    /// Fit happens on the first frame, when the viewport's real size exists.
    fitted: bool,
    /// The focus act was asked for; it is answered in the viewport, where
    /// the size the fit needs exists (Phase 41 slice 1).
    focus_pending: bool,
    /// The camera move in flight: from, to, and when it began. Stepped by
    /// `view::Camera::toward` each frame; any hand on the camera ends it.
    tween: Option<(Camera, Camera, std::time::Instant)>,
    /// The grip being dragged (Phase 41 slice 4) and where the drag began
    /// (viewport screen coordinates, +y up). While it is held the orbit
    /// and the pick stand aside; on release the draft is written.
    grip: Option<(view::Handle, (f64, f64))>,
    /// The viewport's last drawn size — the map's indicator asks what the
    /// main camera can see.
    vp_size: (f32, f32),
    /// Whether the gesture in progress is the map's — decided once when it
    /// began (`view::map_owns_gesture`), `None` between gestures.
    map_gesture: Option<bool>,
    /// The extracted surface, shared with the paint callback — `Arc` because
    /// the callback outlives the frame that built it and must not copy a
    /// four-megabyte mesh to say so.
    sim: Option<Result<std::sync::Arc<view::StockSurface>, String>>,
    sim_rx: Option<std::sync::mpsc::Receiver<Result<std::sync::Arc<view::StockSurface>, String>>>,
    /// The facts with a place, painted after the toolpaths — the findings' (lane A), indexed from `rows`.
    annotations: Vec<view::Annotation>,
    /// The findings pane's rows — the check's check result, the error first.
    rows: Vec<view::FindingRow>,
    /// The check's account, under the rows and on the Problems line.
    account: Vec<crate::check_result::FamilyAccount>,
    /// The emitted program, line by line, each line knowing which op wrote
    /// it — or the engine's error, which is the other thing the terminal
    /// prints and so the other thing the pane says.
    gcode: Result<view::GcodeView, String>,
    /// The whole program's clock (Phase 50 slice 8), the transport bar's line.
    clock: Result<crate::cut_time::TimeReport, String>,
    /// The emission emitted moves, shared with the worker threads that build the
    /// prefix field and the rest cells.
    emitted_moves: std::sync::Arc<Vec<OpEmission>>,
    /// The cursor: how many ops have run. Toolpaths through op `k`, the
    /// declared stock after `k`, the swept stock after `k`, the cells at
    /// `k`. Starts at every op — the program as a whole.
    op_cursor: usize,
    /// Where the cursor stands once the reload of an operation just landed
    /// MID-program arrives (Phase 61 slice 3): ON the new operation, so it
    /// is run and drawn and the next skeleton of a row lands after it.
    cursor_after_write: Option<usize>,
    /// What a Create just made, selected once its reload arrives (ROADMAP
    /// item 3 slice 1): the form closes on Create and the new thing's card,
    /// with its fields, is the next thing seen.
    select_after_write: Option<Made>,
    /// The block cursor nested inside the op cursor (Phase 16 slice 3,
    /// DESIGN.md §5.6): which of the highlighted op's moves is active.
    /// `set_op_cursor` clears it — the nesting invariant — so only the
    /// block acts and a click on a move line can set it.
    block_cursor: Option<usize>,
    /// One frame's request to bring the active block into view: set by the
    /// block KEYS only — a click never scrolls, the reader is already
    /// there (DESIGN.md §5.3).
    scroll_program_to_block: bool,
    /// One frame's request to bring an operation's first line into view:
    /// set by an Outline click (the reader is NOT there yet — the live run
    /// of 2026-09-04: the program followed a click on itself but never the
    /// Outline), never by a click on the program.
    scroll_program_to_op: Option<usize>,
    /// The Outline's request (its tree and its run order) to bring an operation's
    /// row into view — a Program click's (DESIGN.md §5.2, 2026-09-17).
    /// Consumed by whichever pane draws next; dropped when neither does.
    scroll_sidebar_to_op: Option<usize>,
    /// The text zoom (egui's zoom factor): the workspace file's `ui.zoom`
    /// applied on the first frame, read back every frame, saved on close.
    zoom: f32,
    zoom_applied: bool,
    /// What the surface said when the window opened (Phase 25 slice 2,
    /// fork 6): `Some(true)` — it can be see-through; `Some(false)` — it
    /// cannot, and a glass theme is worn over black; `None` until the
    /// selector has run. Written by `select_adapter`, read by the status
    /// line.
    glass_granted: std::sync::Arc<std::sync::Mutex<Option<bool>>>,
    /// What the platform did about the window's own frame (Phase 57 slice
    /// 1), set at creation by `apply_frame`, read by the status line.
    frame_dwm: view::FrameDwm,
    /// Viewport commands an act asked for, sent at the next frame's start:
    /// an act has no context in hand, the frame does.
    pending_viewport: Vec<egui::ViewportCommand>,
    /// Whether the window is maximized, read each frame: the caption's
    /// maximize glyph says restore then, and the act restores.
    maximized: bool,
    /// What became of the theme's `blurred` (slice 3): set once at window
    /// creation by `apply_blur`, read by the status line.
    blur: view::Blur,
    /// The declared stock's edges at `k`, or the model's reason for
    /// declining — rebuilt when `k` moves.
    removal: (usize, Result<Vec<Vec<[f64; 3]>>, String>),
    /// The rest cells' switch and result, keyed by the `k` they were built
    /// for; `cells_rx` is the worker in flight.
    cells: Option<(usize, Result<std::sync::Arc<view::RestCellsView>, String>)>,
    /// The immersion field in hand, keyed by the op whose floor it is
    /// (Phase 41 slice 2) — derived from the project, dropped with it on
    /// a reload; built on the frame the layer first asks, synchronously
    /// (a floor's grid is thousands of cells, not millions).
    immersion: Option<(usize, Result<crate::engagement::Immersion, String>)>,
    /// The field as drawn (ROADMAP item 3 slice 3), keyed like `immersion`:
    /// squares within `view::IMMERSION_DRAW_BUDGET`, built once per field.
    immersion_blocks: Option<(usize, view::ImmersionBlocks)>,
    cells_rx: Option<std::sync::mpsc::Receiver<(usize, Result<std::sync::Arc<view::RestCellsView>, String>)>>,
    /// Which `k` the sim surface in hand was built for.
    sim_cursor: usize,
    /// The block the surface in hand was built through (Phase 52 slice 6):
    /// stale when the block cursor moves, as `sim_cursor` is for the op.
    sim_block: Option<usize>,
    /// The one selection (DESIGN.md I6): written by `set_selection` and
    /// nowhere else; the lit op, the lit feature, the tool's position, the
    /// camera's target and the inspector all derive from it in `view`.
    selection: view::Selection,
    /// The fixture's keep-outs, derived once.
    fixture: Vec<Vec<[f64; 3]>>,
    /// The `?` overlay.
    help: bool,
    /// The slot table every paint below reads — from
    /// slice 3 a theme FILE's slots, judged through `theme::apply_policy`.
    /// The shell constructs no colour of its own: a slot, an alpha,
    /// nothing else.
    theme: Theme,
    /// What `--theme` (or the workspace file) named — written back on
    /// close so the choice survives the window.
    theme_choice: Option<String>,
    /// The watched theme file and its last seen mtime: a change reloads
    /// the theme live, a failed reload names its reason and keeps the
    /// last good one.
    theme_path: Option<std::path::PathBuf>,
    theme_mtime: Option<std::time::SystemTime>,
    /// The window's own theme line — the pane half of `--theme-check`'s
    /// voice: an active theme says nothing, a rejected or unreadable one says
    /// so where the user is looking, in one sentence.
    theme_note: Option<String>,
    /// The project this window was opened on and follows (DESIGN.md §1):
    /// the path, the newest mtime over every file the parse opened as of
    /// the last (attempted) load, the standing refusal of the last reload
    /// that failed — cleared by the next that succeeds — and the palette's
    /// "reload now", which forgets the mtime so the watch fires next frame.
    project_path: String,
    project_mtime: Option<std::time::SystemTime>,
    project_note: Option<String>,
    project_reload: bool,
    /// A reload in flight (ROADMAP item 3 slice 2): the worker that loads
    /// and checks the project, whether it is a switch, and the mtime it
    /// was spawned on — put back after the adopt, so a write that landed
    /// while it ran is seen by the next poll and not swallowed.
    reload_rx: Option<(std::sync::mpsc::Receiver<Result<(Part, CheckResult), String>>, bool, Option<std::time::SystemTime>)>,
    /// The writes this window made (DESIGN.md §1, the one write), and the
    /// standing note of the last write, undo or redo that was refused —
    /// cleared by the next that succeeds, or by a reload.
    edits: view::EditHistory,
    edit_note: Option<String>,
    /// The runner's refusal of its line ("Not run — …"), alive as long as
    /// that line: the next Enter or edit of the line clears it (Phase 56
    /// slice 2, fork 2A — Phase 54's walk saw one nag after every change
    /// while it sat in `edit_note`, cleared only by an act that succeeds).
    runner_refusal: Option<String>,
    /// What Output's line has taken, for ↑ and ↓ (Phase 59 nudge 2).
    runner_history: view::LineHistory,
    /// A transient note (DESIGN.md §5.7): what the last write did, or
    /// "Template copied" — stands `view::TRANSIENT_SECS` and clears on its
    /// own. Kept apart from `edit_note` because the reload a write causes
    /// clears that one, and the confirmation must outlive it.
    transient: Option<(String, std::time::Instant)>,
    /// A copy asked of `act`, done where the context is (the focus
    /// flag's pattern): `program.copy`, DESIGN.md §5.3.
    program_copy_pending: bool,
    /// A project just switched to by a drop (Phase 50 slice 3): a load
    /// that fails lands in a stance, not in "Not reloaded".
    project_switch: bool,
    /// The creation form's quick-start (Phase 50 slice 6): w, l, dia, x, y.
    quick_shape: [String; 5],
    /// The feature under the pointer (Phase 50 slice 9): what a click
    /// would pick, drawn dimmer in the selection colour.
    hover_feature: Option<usize>,
    /// An import in flight (Phase 50 slice 10): the STEP's name for the
    /// status line, and the worker's channel.
    import_busy: Option<String>,
    import_rx: Option<std::sync::mpsc::Receiver<Result<(String, Result<String, String>), String>>>,
    /// Per review row, the earlier row it repeats (`view::duplicate_proposals`).
    review_dups: Vec<Option<usize>>,
    /// The `# run` lines of the project's source files (DESIGN.md §5.2,
    /// the Outline's Suppressed heading) — read from the files at open and
    /// at every reload, never from the program.
    suppressed: Vec<view::SuppressedOp>,
    /// The fields being typed in (DESIGN.md §5.4): each its target, key
    /// and the text so far — a draft in one field stands while another is
    /// typed in. Cleared by a selection change and by a reload; a field not
    /// being edited shows the file's value, never a remembered one.
    field_edit: Vec<(FieldTarget, String, String)>,
    /// The selected op's pattern table (Phase 28 slice 5) — `view::pattern_table`
    /// runs the emitter once per pattern, so it is asked for when the selection
    /// or the project changes and kept here meanwhile: (op, the text it was read
    /// from, the table). Built on a worker thread like the sim surface and the
    /// rest scan: on a rest-floor program the measurement is tens of seconds,
    /// and it used to run inside the paint, which froze the window for that
    /// long on every click (the user's report, 2026-09-29).
    pattern_table: Option<(usize, String, Option<view::PatternTable>)>,
    /// The table in flight: the worker that will hand back the entry above.
    pattern_rx: Option<std::sync::mpsc::Receiver<(usize, String, Option<view::PatternTable>)>>,
    /// The selected op's walk as emitted (ROADMAP item 3 slice 4), keyed by
    /// the op and its line: drawn over its path and said on its card.
    walk: Option<(usize, String, Option<crate::compare::Walked>)>,
    walk_rx: Option<std::sync::mpsc::Receiver<(usize, String, Option<crate::compare::Walked>)>>,
    /// The creation form (DESIGN.md §1, the seventh write) — `Some` while
    /// it holds the inspector; only unsent draft text lives here.
    new_form: Option<NewForm>,
    /// The review (DESIGN.md §5.2): `report.json` in the project's
    /// directory, followed by mtime like every file this window reads —
    /// `None` when there is none, `Some(Err)` when it will not read — its
    /// rows rebuilt from the report and the program at every reload of
    /// either, and the selected proposal's own edges, derived at selection.
    report_path: std::path::PathBuf,
    report_mtime: Option<std::time::SystemTime>,
    report: Option<Result<crate::review::Report, String>>,
    review_rows: Vec<view::ReviewRow>,
    /// The statements as edited on the Review card, by face id (ROADMAP
    /// item 1, slice 3): what Accept writes in place of the report's text.
    /// Cleared with the report; one that equals the report's is no edit.
    review_drafts: std::collections::BTreeMap<String, String>,
    /// The sidecar's `proposed_stock.part` beside the report, watched the
    /// same way; the row the Review lists first, derived from the file and
    /// the program at every refresh. `stock_declined` is the sidecar's
    /// sentence when this window's last run proposed none.
    stock_mtime: Option<std::time::SystemTime>,
    stock_text: Option<String>,
    stock_proposal: Option<Result<view::StockProposal, String>>,
    stock_declined: Option<String>,
    proposal_edges: Option<Vec<FeatureEdges>>,
    /// A move-less copy of the check result, kept so a theme reload can rebake
    /// `rows` and `annotations` with the new slots.
    check_result: CheckResult,
    /// The arrangement: which sidebar view shows, which document the
    /// center shows, which walls are open and how wide, which layers are
    /// on. The shell never edits any of it directly: input becomes a
    /// `DockAction` through `Layout::apply`, where the pins are. Read from
    /// the workspace file at open, written back at close.
    layout: Layout,
    /// back verbatim at close, so a rebinding survives the window even
    /// when this session never touched it.
    raw_keys: Vec<(String, String)>,
    /// The workspace's `shell <program>` (Phase 59 slice 4, fork 4A),
    /// read for the runner's `sh` and written back as read.
    shell: Option<String>,
    /// The keyboard, resolved: action id → key, the
    /// defaults re-bound by the file's judged lines. Read-only after
    /// build; every press becomes `act(id)` — the ONE dispatcher the
    /// palette Enter cannot disagree about what an action does.
    keymap: Vec<(&'static str, egui::Key)>,
    /// The quick-action palette: open, its filter, its selection.
    palette_open: bool,
    palette_query: String,
    palette_sel: usize,
    /// Whether the window asked for a see-through surface when it was MADE
    /// (Phase 51 slice 5): `with_transparent` is a creation-time choice, so
    /// a theme picked later whose appearance crosses that line is honest
    /// about taking effect at the next launch (`view::theme_pick_note`).
    glass_asked_at_launch: bool,
    /// The overlay opened THIS frame (Phase 51 slice 2, I14): the click
    /// that opened the palette or the help lands outside the window it
    /// opens, in the same frame — so that one click is not read as the
    /// click outside that closes it. Cleared at the frame's end.
    overlay_opened_this_frame: bool,
    /// The Layers list was drawn last frame (Phase 52 slice 2): only then
    /// does the pointer over its rect keep it open — egui remembers an
    /// area's rect after it stops being shown.
    layers_list_shown: bool,
    /// When the pointer came to rest on the Layers button or its list
    /// (Phase 52 slice 11), on egui's own clock (`input.time`: wall time
    /// under eframe, a quarter second a frame under the harness): the list
    /// drops once it has rested `view::LAYERS_HOVER_DELAY`, read through
    /// `view::layers_open`; `None` while the pointer is elsewhere.
    layers_hover_since: Option<f64>,
    /// The Program's pane and the scroll area drawn in it, last frame — for
    /// the harness: the document must reach the minimap strip (slice 3).
    program_rects: Option<(egui::Rect, egui::Rect)>,
    /// The width the Program row's controls took last frame (Phase 52
    /// slice 12): the label is given what they leave, so the row is laid
    /// left to right and a pane narrower than its controls overflows to
    /// the RIGHT, clipped, and never past the pane's left edge.
    program_controls_w: f32,
    /// The center's width last frame, and the split panel's epoch (Phase
    /// 59 nudge 1): when the center's width changes by anything but a
    /// drag on the split — a wall dragged, the window resized — the
    /// viewport panel is re-keyed so the saved RATIO sizes it again,
    /// not egui's memory of its last pixel width; the Program, the width
    /// the viewport leaves, keeps its share.
    center_total: f32,
    split_epoch: u32,
    /// The width the sidebar's close took last frame (Phase 59 nudge 3):
    /// the tabs are given what it leaves, so a tab strip wider than the
    /// pane clips at the pane's edge instead of widening the pane.
    sidebar_close_w: f32,
    /// The width the Viewport row's view strip took last frame (Phase 53
    /// slice 3): the row is laid left to right with the strip pushed to
    /// the right by what the Layers button and the legend leave, so a
    /// viewport narrower than its row overflows to the RIGHT, clipped —
    /// the Program row's cure (Phase 52 slice 12), the same mechanism.
    viewport_strip_w: f32,
    /// The Output log (DESIGN.md §5.8, Phase 54 slice 1): every sentence
    /// the status line has posted, with the second it appeared — posted by
    /// the status bar itself, the one home, so the log cannot disagree
    /// with the line.
    output: view::OutputLog,
    /// When the window opened: the log's clock.
    opened_at: std::time::Instant,
    /// The same moment on the wall clock, for the saved log's header.
    opened_unix: u64,
    /// A copy of the log asked of `act`, done where the context is.
    output_copy_pending: bool,
    /// The refusals on the status line and when each was first seen: one
    /// stands `view::REFUSAL_SECS`, then leaves the line for the log.
    notes_seen: Vec<(&'static str, String, std::time::Instant)>,
    /// The runner's line (DESIGN.md §5.8, Phase 54 slice 2): what the user
    /// has typed at the dock's bottom; Enter runs it.
    runner_line: String,
    /// A run in flight: its lines as they come (stdout and stderr alike),
    /// then its exit. One run at a time.
    runner_rx: Option<std::sync::mpsc::Receiver<String>>,
    /// The run's echo while it runs, for the status line's busy word.
    runner_busy: Option<String>,
    /// The run's process (Phase 59 slice 5): the app's, so Stop can kill
    /// it and the frame can read its exit.
    runner_child: Option<std::process::Child>,
    /// Stop was pressed on this run: its exit line is "killed".
    runner_killed: bool,
    /// The dock just opened: the runner's line takes the keyboard once.
    runner_focus_pending: bool,
    /// The Target group's path field (Phase 52 slice 4): `Some` while it is
    /// open, the text typed or pasted so far. A dialog is a crate or a
    /// platform call — named for the CLOSE; a path, or a drop, imports.
    target_path_draft: Option<String>,
    /// The import's setup as the field holds it (ROADMAP item 1, slice 4):
    /// the up axis and the solid, kept across imports in this window so a
    /// second try changes one word. `import_last` is the path of the import
    /// running, put back in the field when the sidecar refuses it.
    import_setup: view::ImportSetup,
    import_last: Option<std::path::PathBuf>,
    /// The tool's pose as last drawn (Phase 52 slice 5) — for the harness.
    pose_drawn: Option<([f64; 3], usize)>,
    /// The grips as last drawn — each key and its screen point (Phase 59
    /// nudge 4) — for the harness, which drags one as a hand would.
    grips_drawn: Vec<(String, egui::Pos2)>,
    /// The repository's head for the band's chip (Phase 52 slice 7): read
    /// on a worker every few seconds, never on the frame.
    git: Option<Result<view::GitHead, String>>,
    git_rx: Option<std::sync::mpsc::Receiver<Result<view::GitHead, String>>>,
    git_read_at: Option<std::time::Instant>,
    /// The open-file dialog's answer, on its way back from the worker
    /// (Phase 52 slice 9): `Some(None)` is a cancel.
    dialog_rx: Option<std::sync::mpsc::Receiver<Option<std::path::PathBuf>>>,
    /// Bumped on revert and on a mode switch: egui remembers a panel's
    /// dragged size by id, so a fresh id is what makes `default_size`
    /// apply again (shift 5 in DESIGN.md) — the model's number wins.
    panel_epoch: u32,
}

/// How close (screen units) a click must land to an edge to pick its
/// feature — a presentation number: finger-loose, not sub-pixel.
const PICK_TOL: f64 = 8.0;
/// A point annotation's cross, half-width in screen units, and its label's
/// offset from it — presentation numbers.
const ANNOTATION_HALF: f32 = 5.0;
const ANNOTATION_LABEL_DX: f32 = 8.0;

/// A wall's frame under the band: egui's side panels keep a 2-pixel top
/// margin and the center keeps 8, so the sidebar's rows sat six pixels
/// above the center's (the harness's finding, Phase 52 slice 2). The
/// center's margin, on every column that draws band rows.
/// The minimap strip's width (DESIGN.md §5.3): a constant, not a drag —
/// pub so the harness can say where the document must reach.
pub const MINIMAP_STRIP_W: f32 = 48.0;

fn band_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::side_top_panel(style).inner_margin(egui::Margin { left: 8, right: 8, top: 8, bottom: 2 })
}

/// The runner's process (Phase 54 slice 2; Phase 59 slice 5, fork 5A):
/// spawned with both pipes and stdin the null device, each pipe read on
/// its own thread line by line into one channel, which closes when both
/// readers are done. The child itself goes back to the app: the frame
/// reads its exit (`try_wait`, once the channel has closed, so the exit
/// is said after every line) and Stop kills it — no thread holds the
/// handle a kill would need. A program that cannot start is one line.
fn spawn_run(spec: &view::RunSpec) -> Result<(std::process::Child, std::sync::mpsc::Receiver<String>), String> {
    use std::io::BufRead;
    // stdin is the null device (Phase 59 slice 4): a program that asks a
    // question reads EOF and ends, instead of waiting on nobody — the
    // window has no console to hand it.
    let mut command = std::process::Command::new(&spec.program);
    command.args(&spec.args).current_dir(&spec.cwd).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    // Its own process group on Unix, so Stop can kill what it started
    // too: `sh sleep 60` is a shell and a sleep, and the sleep kept the
    // pipe open after the shell alone was killed.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", spec.program.display()))?;
    let (tx, rx) = std::sync::mpsc::channel();
    if let Some(out) = child.stdout.take() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            // cmd's lines end "\r\n"; `lines()` takes the "\n" alone.
            for l in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                let _ = tx.send(l.trim_end_matches('\r').to_string());
            }
        });
    }
    if let Some(err) = child.stderr.take() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for l in std::io::BufReader::new(err).lines().map_while(Result::ok) {
                let _ = tx.send(l.trim_end_matches('\r').to_string());
            }
        });
    }
    // The readers hold the only senders now: the channel closes with them.
    drop(tx);
    Ok((child, rx))
}

/// An edge's cursor (Phase 57 slice 2): egui's own eight.
fn edge_cursor(e: view::Edge) -> egui::CursorIcon {
    match e {
        view::Edge::North => egui::CursorIcon::ResizeNorth,
        view::Edge::South => egui::CursorIcon::ResizeSouth,
        view::Edge::East => egui::CursorIcon::ResizeEast,
        view::Edge::West => egui::CursorIcon::ResizeWest,
        view::Edge::NorthEast => egui::CursorIcon::ResizeNorthEast,
        view::Edge::NorthWest => egui::CursorIcon::ResizeNorthWest,
        view::Edge::SouthEast => egui::CursorIcon::ResizeSouthEast,
        view::Edge::SouthWest => egui::CursorIcon::ResizeSouthWest,
    }
}

/// An edge's resize direction, the platform's word for it.
fn edge_direction(e: view::Edge) -> egui::ResizeDirection {
    match e {
        view::Edge::North => egui::ResizeDirection::North,
        view::Edge::South => egui::ResizeDirection::South,
        view::Edge::East => egui::ResizeDirection::East,
        view::Edge::West => egui::ResizeDirection::West,
        view::Edge::NorthEast => egui::ResizeDirection::NorthEast,
        view::Edge::NorthWest => egui::ResizeDirection::NorthWest,
        view::Edge::SouthEast => egui::ResizeDirection::SouthEast,
        view::Edge::SouthWest => egui::ResizeDirection::SouthWest,
    }
}

/// One row of the band (DESIGN.md §4, §5.3 — Phase 52 slice 2): the
/// column's full width at `view::BAND_ROW_H`, its widgets centred, so
/// every column's rows share a top and a bottom whatever they hold.
fn band_row(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), view::BAND_ROW_H), egui::Layout::left_to_right(egui::Align::Center), add);
}

fn rgb(c: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

/// A slot with a translucency — the alpha is the shell's presentation
/// number; the colour is never the shell's.
fn rgba(c: [u8; 3], a: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], a)
}

/// THE one function that builds an `egui::Visuals` — every chrome colour
/// the window uses comes from a theme slot through this mapping and
/// nowhere else. It starts from egui's stock dark
/// visuals and overrides exactly the fields the slot table names; with
/// `Theme::default()` every override writes back the stock value, so the
/// default window is the window the first panes shipped with. The interactive text and
/// element ladders are set per widget state because egui draws them per
/// state — the slot names are the theme's vocabulary for that ladder.
fn visuals(t: &Theme) -> egui::Visuals {
    // A fill wears the alpha the file wrote for it (Phase 25 slice 2,
    // fork 2); a stroke and a text never do. 255 is the opaque window as
    // shipped.
    let f = t.fill_alpha;
    // The theme's own word for what its chrome is laid over (2026-09-17):
    // a light theme starts from egui's light visuals, so the widgets the
    // slot table does not name — a tooltip's shadow, a hyperlink — are
    // lit the same way as the slots that are.
    let mut v = match t.appearance {
        crate::theme::Appearance::Dark => egui::Visuals::dark(),
        crate::theme::Appearance::Light => egui::Visuals::light(),
    };
    v.window_fill = rgba(t.background, f.background);
    v.window_stroke.color = rgb(t.border);
    v.panel_fill = rgba(t.panel_background, f.panel_background);
    v.widgets.noninteractive.bg_fill = rgba(t.panel_background, f.panel_background);
    v.widgets.noninteractive.weak_bg_fill = rgba(t.panel_background, f.panel_background);
    v.widgets.noninteractive.bg_stroke.color = rgb(t.border);
    v.widgets.noninteractive.fg_stroke.color = rgb(t.text);
    v.widgets.inactive.bg_fill = rgba(t.element, f.element);
    v.widgets.inactive.weak_bg_fill = rgba(t.element, f.element);
    v.widgets.inactive.fg_stroke.color = rgb(t.text_interactive);
    v.widgets.hovered.bg_fill = rgba(t.element_hover, f.element_hover);
    v.widgets.hovered.weak_bg_fill = rgba(t.element_hover, f.element_hover);
    v.widgets.hovered.bg_stroke.color = rgb(t.border_hover);
    v.widgets.hovered.fg_stroke.color = rgb(t.text_hover);
    v.widgets.active.bg_fill = rgba(t.element_active, f.element_active);
    v.widgets.active.weak_bg_fill = rgba(t.element_active, f.element_active);
    v.widgets.active.bg_stroke.color = rgb(t.text_active);
    v.widgets.active.fg_stroke.color = rgb(t.text_active);
    v.widgets.open.bg_fill = rgba(t.panel_background, f.panel_background);
    v.widgets.open.weak_bg_fill = rgba(t.element_open, f.element_open);
    v.widgets.open.bg_stroke.color = rgb(t.border);
    v.widgets.open.fg_stroke.color = rgb(t.text_open);
    v.selection.bg_fill = rgba(t.element_selected, f.element_selected);
    // The two slots the review found unthemed: the text field's ground
    // (egui's `extreme_bg_color`, which a TextEdit and a scroll area's
    // trough both wear) and the scrollbar's handle (the inactive
    // widget's `fg_stroke`, which egui's handle wears at rest).
    v.extreme_bg_color = rgba(t.text_field, f.text_field);
    v.text_edit_bg_color = Some(rgba(t.text_field, f.text_field));
    v.widgets.inactive.fg_stroke.color = rgb(t.scrollbar);
    v
}

/// The window's one scroll style (Phase 59 slice 2, fork 1A): egui's
/// solid bars with the handle on the foreground slot, where `visuals()`
/// puts the theme's `scrollbar` colour. Read by `ui()` every frame and by
/// the harness, which measures every bar against `bar_width`.
pub fn scroll_style() -> egui::style::ScrollStyle {
    egui::style::ScrollStyle { foreground_color: true, ..egui::style::ScrollStyle::solid() }
}

/// The frame benchmark (`NACS_FRAME_BENCH`): per section, the mean time
/// inside `ui()`; and the wall time from one frame's start to the next, which
/// is everything — layout, tessellation, the GPU, presentation.
struct FrameBench {
    frames: usize,
    seen: usize,
    last_start: Option<std::time::Instant>,
    mark: std::time::Instant,
    wall: Vec<f64>,
    sections: Vec<(&'static str, f64)>,
}

impl FrameBench {
    fn new(frames: usize) -> FrameBench {
        FrameBench { frames, seen: 0, last_start: None, mark: std::time::Instant::now(), wall: Vec::new(), sections: Vec::new() }
    }

    fn frame_begin(&mut self, now: std::time::Instant) {
        if let Some(prev) = self.last_start {
            self.wall.push((now - prev).as_secs_f64() * 1000.0);
        }
        self.last_start = Some(now);
        self.mark = now;
    }

    fn mark(&mut self, name: &'static str) {
        let now = std::time::Instant::now();
        let ms = (now - self.mark).as_secs_f64() * 1000.0;
        self.mark = now;
        // The first few frames load and bake; they are not the steady state.
        if self.seen < 5 {
            return;
        }
        match self.sections.iter_mut().find(|(n, _)| *n == name) {
            Some(s) => s.1 += ms,
            None => self.sections.push((name, ms)),
        }
    }

    /// True when the run is done (and the report printed).
    fn frame_end(&mut self) -> bool {
        self.seen += 1;
        // (Once: the close lands a frame later.)
        if self.seen != self.frames + 5 {
            return false;
        }
        let n = (self.seen - 5) as f64;
        let steady: Vec<f64> = self.wall.iter().skip(5).copied().collect();
        let mean = steady.iter().sum::<f64>() / steady.len().max(1) as f64;
        let mut sorted = steady.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p95 = sorted.get(((sorted.len() as f64) * 0.95) as usize).copied().unwrap_or(0.0);
        eprintln!("frame bench: {} frames, wall mean {:.1} ms ({:.0} fps), p95 {:.1} ms", steady.len(), mean, 1000.0 / mean.max(1e-9), p95);
        let inside: f64 = self.sections.iter().map(|(_, t)| t).sum::<f64>() / n;
        for (name, t) in &self.sections {
            eprintln!("  {:<18} {:>7.2} ms", name, t / n);
        }
        eprintln!("  {:<18} {:>7.2} ms  (inside ui(); the rest of the wall time is layout, tessellation, GPU and presentation)", "sum", inside);
        true
    }
}

impl ViewApp {
    fn bench_mark(&mut self, name: &'static str) {
        if let Some(b) = self.bench.as_mut() {
            b.mark(name);
        }
    }
}

impl eframe::App for ViewApp {
    /// What the swapchain is cleared to before egui paints: the theme's
    /// `background` at its own alpha — the one hook eframe gives for the
    /// pixels behind every panel (its doc: gamma-space, as-is; a `Color32`
    /// is already premultiplied, which a see-through surface wants). Where
    /// a file makes every pane `#00000000` — Zed's blur themes — this is
    /// the one sheet of glass everything sits on. Opaque themes clear to
    /// what they always did.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let c = self.theme.background;
        egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], self.theme.fill_alpha.background).to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // The window as it stands, and the acts' commands (Phase 57 slice
        // 1): an act has no context in hand, so it queues, and the frame
        // sends first thing.
        self.maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        for cmd in std::mem::take(&mut self.pending_viewport) {
            ui.ctx().send_viewport_cmd(cmd);
        }
        // Hot reload: a watched theme file's mtime is
        // polled per frame — with a slow repaint timer so an idle window
        // still notices — and a change re-reads, re-judges and re-uses
        // the theme, rebaking the rows and annotations whose colours were baked
        // with the old slots. A failed reload named its reason in
        // `load_theme_file` and the last good theme stays on.
        // The window follows the file (DESIGN.md §1): the project's files
        // are polled on the same slow timer as the theme, and a change
        // reloads — one path, whoever wrote the file.
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        // The way out's copy (DESIGN.md §5.3, Phase 50 slice 2): `act` holds
        // no context, so the clipboard is written here, once.
        if std::mem::take(&mut self.output_copy_pending) {
            ui.ctx().copy_text(self.output.export(&self.project_path, self.opened_unix));
            self.transient = Some(("Output copied".to_string(), std::time::Instant::now()));
        }
        if self.program_copy_pending {
            self.program_copy_pending = false;
            if let Ok(g) = &self.gcode {
                ui.ctx().copy_text(view::program_text(&g.text()));
                self.transient = Some(("Program copied".to_string(), std::time::Instant::now()));
            }
        }
        // A folder dropped on the window opens it (DESIGN.md §1, Phase 50
        // slice 3): a drop is not a dialog. The first dropped path that is
        // a project — a directory, or a .part file's directory — wins.
        let dropped: Vec<std::path::PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().map(|d| d.path().to_path_buf()).collect());
        if let Some(step) = dropped.iter().find(|p| view::is_step(p)).cloned() {
            // A STEP dropped on the window imports it here (DESIGN.md §5.2,
            // Phase 50 slice 10): the sidecar on a worker, the Review pane
            // following report.json as it always has.
            self.start_import(step);
        } else if let Some(project) = view::dropped_project(&dropped) {
            self.switch_project(project, ui.ctx());
        }
        // The run's lines land in the log as they come (Phase 54 slice 2);
        // a repaint is asked while it runs so they are seen as they land.
        // The exit is the frame's to read (Phase 59 slice 5, fork 5A): the
        // child is the app's, so Stop can reach it, and its exit is said
        // once the channel has closed (both readers done, every line in)
        // AND `try_wait` says the process is gone — so the exit is last,
        // and a line that outlives its process (a child of the shell's
        // holding the pipes) is still waited for.
        if let Some(rx) = &self.runner_rx {
            let t = self.opened_at.elapsed().as_secs_f64();
            let mut pipes_closed = false;
            loop {
                match rx.try_recv() {
                    Ok(l) => self.output.append(t, &l),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        pipes_closed = true;
                        break;
                    }
                }
            }
            let status = if pipes_closed { self.runner_child.as_mut().and_then(|c| c.try_wait().ok().flatten()) } else { None };
            let done = match status {
                Some(s) => {
                    let line = if self.runner_killed {
                        "killed".to_string()
                    } else {
                        match s.code() {
                            Some(0) => "exit 0".to_string(),
                            Some(c) => format!("exit {c}"),
                            None => "ended by a signal".to_string(),
                        }
                    };
                    self.output.append(t, &line);
                    self.runner_child = None;
                    self.runner_killed = false;
                    true
                }
                None => false,
            };
            if done {
                // A git run may have moved the branch or the tree: the chip
                // reads again now, not on its three-second tick.
                if self.runner_busy.as_deref().is_some_and(|e| e.starts_with("git ")) {
                    self.git_read_at = None;
                }
                // A stock run that ended well left proposed_stock.part: the
                // accept (slice 5b), through the one write path.
                let stock_run = self.runner_busy.as_deref().is_some_and(|e| e.starts_with("stock "));
                let exit_zero = self.output.events.last().is_some_and(|e| e.text.trim() == "exit 0");
                self.runner_rx = None;
                self.runner_busy = None;
                if stock_run {
                    // The proposal is reviewed like every other (ROADMAP
                    // item 1): the Review lists it first, Accept writes it.
                    self.stock_declined = if exit_zero {
                        None
                    } else {
                        self.output.events.iter().rev().find_map(|e| e.text.trim().strip_prefix("declined: ").map(|s| s.to_string())).or_else(|| Some("the sidecar refused; its lines are in Output".to_string()))
                    };
                    self.show_proposals();
                }
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        if let Some(rx) = &self.import_rx {
            if let Ok(result) = rx.try_recv() {
                self.import_rx = None;
                self.import_busy = None;
                match result {
                    Ok((summary, stock)) => {
                        let faces = summary.lines().last().unwrap_or("report.json written").to_string();
                        let blank = match &stock {
                            Ok(_) => "the stock proposed".to_string(),
                            Err(why) => format!("no stock proposed: {why}"),
                        };
                        self.stock_declined = stock.err();
                        self.transient = Some((format!("Imported — {faces}; {blank}"), std::time::Instant::now()));
                        self.import_last = None;
                        self.show_proposals();
                    }
                    Err(e) => {
                        self.edit_note = Some(format!("Not imported — {e}"));
                        // The path goes back in the field: a refusal that
                        // names the solids wants one word changed, not the
                        // path typed again.
                        if let Some(step) = self.import_last.take() {
                            self.target_path_draft = Some(step.to_string_lossy().into_owned());
                            self.layout.apply(DockAction::ShowTab(SidebarTab::Outline));
                        }
                    }
                }
            }
        }
        // The saved zoom, once; then the live one, every frame, so Ctrl
        // +/− is what the file remembers (DESIGN.md §5.7, 2026-09-17).
        if !self.zoom_applied {
            ui.ctx().set_zoom_factor(self.zoom);
            self.zoom_applied = true;
        } else {
            self.zoom = ui.ctx().zoom_factor();
        }
        let frame_start = std::time::Instant::now();
        if let Some(b) = self.bench.as_mut() {
            let first = b.seen == 0;
            b.frame_begin(frame_start);
            ui.ctx().request_repaint();
            // `NACS_FRAME_BENCH_SEL=op:0 | feature:2 | finding:0 | tool:5 |
            // stock`: measure with that selected, which is a different frame —
            // the inspector is open on it, and each kind builds its own card.
            if first {
                // `NACS_FRAME_BENCH_TAB=outline|sequence|problems|review`:
                // only the open tab is built, so each is its own frame.
                if let Ok(tab) = std::env::var("NACS_FRAME_BENCH_TAB") {
                    self.layout.sidebar.tab = match tab.as_str() {
                        "problems" => crate::theme::SidebarTab::Problems,
                        "review" => crate::theme::SidebarTab::Review,
                        _ => crate::theme::SidebarTab::Outline,
                    };
                }
                if let Ok(sel) = std::env::var("NACS_FRAME_BENCH_SEL") {
                    let (kind, n) = sel.split_once(':').unwrap_or((sel.as_str(), "0"));
                    let n: usize = n.parse().unwrap_or(0);
                    let sel = match kind {
                        "op" => {
                            self.set_op_cursor(n);
                            view::Selection::Operation(n)
                        }
                        "feature" => view::Selection::Feature(n),
                        "finding" => view::Selection::Finding(n),
                        "tool" => view::Selection::Tool(n),
                        _ => view::Selection::Stock,
                    };
                    self.set_selection(sel);
                }
            }
        }
        self.poll_project(ui.ctx());
        self.poll_report();
        self.poll_stock_proposal();
        self.poll_git();
        self.poll_dialog();
        self.bench_mark("poll");
        if let Some(path) = self.theme_path.clone() {
            let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
            if mtime != self.theme_mtime {
                self.theme_mtime = mtime;
                let name = self.theme_choice.clone().unwrap_or_else(|| "theme".to_string());
                let (theme, note) = load_theme_file(&path, &name);
                self.apply_theme(theme, note);
            }
        }
        // The chrome uses the theme, re-applied per frame — idempotent
        // between reloads, and the seam a reload changes it through.
        ui.ctx().set_visuals(visuals(&self.theme));
        // ONE scroll style for the window (Phase 59 slice 2, fork 1A),
        // installed at the same seam: egui's solid bars — they take their
        // room from the area, stand whenever the content overflows, and
        // drag (Phase 52 slice 15's reason, a mouse without a side wheel)
        // — with the handle on the FOREGROUND slot, so every bar wears
        // the theme's `scrollbar` colour, the one a theme file names and
        // `--theme-check` judges. The Program alone wore the solid style
        // before, and solid's default puts the handle on
        // `widgets.inactive.bg_fill` — the theme's `element` — which is
        // why its bars read more subdued than the form's floating ones
        // (the Phase 58 walk's item 2).
        ui.ctx().all_styles_mut(|s| s.spacing.scroll = scroll_style());
        // The keys: every binding in the resolved
        // keymap fires `act(id)` — the ONE dispatcher. Quiet while the
        // palette is open or any widget holds keyboard focus (typing a
        // filter or a field's value must not fire actions — the live
        // run's finding, 2026-08-30); the palette's own window handles
        // Esc, Enter and the arrows.
        // A chord is not a key: `key_pressed` matches whatever modifiers
        // are down, and egui's own Ctrl+0 (zoom reset, judged after this
        // frame) fired `0` — the arrangement reverted and the workspace
        // file rewritten. Shift stays allowed: `{`, `}` and `?` are shifted.
        let chord = ui.input(|i| i.modifiers.ctrl || i.modifiers.alt || i.modifiers.mac_cmd || i.modifiers.command);
        if !self.palette_open && !ui.ctx().egui_wants_keyboard_input() && !chord {
            let mut fired: Option<&'static str> = None;
            for (id, key) in &self.keymap {
                if ui.input(|i| i.key_pressed(*key)) {
                    fired = Some(id);
                }
            }
            if let Some(id) = fired {
                self.act(id);
            }
        }
        // The named chords (DESIGN.md §9, 2026-09-17): Ctrl+Z and Ctrl+Y
        // are undo's and redo's second names — matched with the command
        // modifier held and nothing else, quiet under keyboard focus as
        // the keys are (a text field's own Ctrl+Z stays its own).
        let command_only = ui.input(|i| i.modifiers.command && !i.modifiers.alt && !i.modifiers.shift);
        if !self.palette_open && !ui.ctx().egui_wants_keyboard_input() && command_only {
            let mut fired: Option<&'static str> = None;
            for (id, binding) in view::ALSO {
                if let Some(name) = view::chord_key(binding) {
                    if egui::Key::from_name(name).is_some_and(|k| ui.input(|i| i.key_pressed(k))) {
                        fired = Some(id);
                    }
                }
            }
            if let Some(id) = fired {
                self.act(id);
            }
        }


        self.bench_mark("head");

        // The band is the title bar (Phase 57 slice 1, fork 0A): under the
        // window's own frame its empty space drags the window and a
        // double-click maximizes or restores — interacted FIRST, so every
        // widget drawn on the band after it wins its own clicks, as egui's
        // own frame example does it.
        if !self.layout.frame_native {
            let screen = ui.max_rect();
            let band = egui::Rect::from_min_size(screen.min, egui::vec2(screen.width(), view::BAND_ROWS as f32 * view::BAND_ROW_H));
            let r = ui.interact(band, egui::Id::new("window.band"), egui::Sense::click_and_drag());
            if r.double_clicked() {
                self.act("window.maximize");
            } else if r.drag_started_by(egui::PointerButton::Primary) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        }

        // The docks, then the viewport as the remainder. A drag on a
        // panel's edge resizes it in egui's hands; the result is read back
        // off the drawn rect and becomes a `Resize` action, so the model
        // is never behind what is on screen (and slice 2 saves the model).
        self.left_wall(ui);
        self.bench_mark("left wall");
        self.bottom_panel(ui);
        self.bench_mark("bottom panel");
        egui::CentralPanel::default().show(ui, |ui| {
            // The status line and the transport bar are pinned under the
            // viewport (DESIGN.md I8, I10): panels inside the center, so
            // nothing the viewport draws can cover them.
            egui::Panel::bottom("status").resizable(false).show(ui, |ui| self.status_bar(ui));
            egui::Panel::bottom("transport").resizable(false).show(ui, |ui| self.transport_bar(ui));
            self.bench_mark("status+transport");
            self.center(ui);
            self.bench_mark("center");
        });
        if self.bench.as_mut().is_some_and(|b| b.frame_end()) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.help {
            let mut close = false;
            let shown = egui::Window::new("help  (?)")
                // Over the layers widget, which is a Foreground area
                // (`layers_widget`): an overlay the viewport's own chrome
                // can cover is not an overlay.
                .order(egui::Order::Foreground)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ui.ctx(), |ui| {
                    // The way out that needs no key (I14, Phase 51 slice 2):
                    // a × on the help's own first line.
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Esc closes · click outside closes").small().weak());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("×").on_hover_text(view::door_hover("dismiss", "close the help")).clicked() {
                                close = true;
                            }
                        });
                    });
                    // Scrolling: the help outgrew the default window, and a
                    // window that cannot scroll hid its own mouse section.
                    let cap = ui.ctx().content_rect().height() * 0.8;
                    // Wide enough that the two-column lines keep their
                    // columns: at egui's default width every line wrapped.
                    ui.set_min_width((ui.ctx().content_rect().width() - 80.0).clamp(320.0, 900.0));
                    egui::ScrollArea::vertical().max_height(cap).show(ui, |ui| {
                        for line in view::help_lines() {
                            if line.is_empty() {
                                ui.add_space(6.0);
                            } else if !line.starts_with(' ') {
                                ui.strong(line);
                            } else {
                                ui.monospace(line);
                            }
                        }
                    });
                });
            // A click outside the help closes it (I14) — unless this is the
            // frame the `?` door opened it in, where that click is the
            // opening one.
            if close || shown.is_some_and(|r| r.response.clicked_elsewhere() && !self.overlay_opened_this_frame) {
                self.act("dismiss");
            }
        }
        // The quick-action palette: one text field,
        // a substring filter over the inventory, Enter or a click runs the
        // selection through the same `act` every key goes through. Zed's
        // shape, nacs's list.
        if self.palette_open {
            let mut run: Option<&'static str> = None;
            let (esc, enter, up, down) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::Escape),
                    i.key_pressed(egui::Key::Enter),
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::ArrowDown),
                )
            });
            let mut close = false;
            let shown = egui::Window::new("quick actions")
                .order(egui::Order::Foreground)
                .title_bar(false)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_TOP, egui::Vec2::new(0.0, 48.0))
                .show(ui.ctx(), |ui| {
                    // The filter, and beside it the way out that needs no
                    // key (I14, Phase 51 slice 2): a ×.
                    let field = ui.horizontal(|ui| {
                        let field = ui.text_edit_singleline(&mut self.palette_query);
                        if ui.small_button("×").on_hover_text(view::door_hover("dismiss", "close the palette")).clicked() {
                            close = true;
                        }
                        field
                    }).inner;
                    field.request_focus();
                    // A narrowed filter re-aims at its first row: a
                    // selection kept by index across a filter change ran a
                    // different act than the one lit before.
                    if field.changed() {
                        self.palette_sel = 0;
                    }
                    let matches = view::palette_matches(&self.palette_query);
                    if up {
                        self.palette_sel = self.palette_sel.saturating_sub(1);
                    }
                    if down {
                        self.palette_sel += 1;
                    }
                    self.palette_sel = self.palette_sel.min(matches.len().saturating_sub(1));
                    let cap = ui.ctx().content_rect().height() * 0.6;
                    egui::ScrollArea::vertical().max_height(cap).show(ui, |ui| {
                        for (i, a) in matches.iter().enumerate() {
                            // The sentence first, the key after (Phase 50 slice 4):
                            // the dotted id is for the filter, not the eye.
                            let row = ui.selectable_label(i == self.palette_sel, egui::RichText::new(view::palette_row(a)).monospace()).on_hover_text(a.sentence);
                            if row.clicked() {
                                run = Some(a.id);
                            }
                            if i == self.palette_sel && (up || down) {
                                row.scroll_to_me(None);
                            }
                        }
                    });
                    if matches.is_empty() {
                        ui.label(egui::RichText::new("no action matches — the list is `?`'s keys section").weak().small());
                    }
                    if enter {
                        run = matches.get(self.palette_sel).map(|a| a.id);
                    }
                });
            // Esc, the ×, or a click outside the palette (I14) — not the
            // click that opened it, which lands outside in the same frame.
            let outside = shown.is_some_and(|r| r.response.clicked_elsewhere() && !self.overlay_opened_this_frame);
            if esc || close || outside {
                self.act("dismiss");
            }
            if let Some(id) = run {
                self.palette_open = false;
                self.act(id);
            }
        }
        // The edges (Phase 57 slice 2, fork 2A): under the window's own
        // frame the OS answers HTCLIENT everywhere (measured in slice 1),
        // so the eight edge zones are the shell's — registered LAST in the
        // frame, so an edge wins over a widget standing in the margin, as
        // the OS's border wins; the cursor says the edge, a drag asks the
        // platform to resize from it. A maximized window has no edges.
        if !self.layout.frame_native && !self.maximized {
            let s = ui.max_rect();
            let window = [s.min.x, s.min.y, s.max.x, s.max.y];
            for edge in view::Edge::ALL {
                let [x0, y0, x1, y1] = edge.rect(window, view::RESIZE_MARGIN);
                let rect = egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1));
                let r = ui.interact(rect, egui::Id::new(("window.edge", edge as u8)), egui::Sense::drag());
                if r.hovered() || r.dragged() {
                    ui.ctx().set_cursor_icon(edge_cursor(edge));
                }
                if r.drag_started_by(egui::PointerButton::Primary) {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::BeginResize(edge_direction(edge)));
                }
            }
        }
        self.overlay_opened_this_frame = false;
    }

    /// The window closes the way it will open: the whole workspace —
    /// temp-file-and-rename, last writer wins, and a failure is a
    /// sentence on stderr — chrome must never block closing.
    fn on_exit(&mut self) {
        if let Err(reason) = crate::theme::save_workspace(&self.workspace_snapshot()) {
            startup_say(format!("workspace not saved: {reason}"));
        }
    }
}

impl ViewApp {
    /// The workspace as this window would save it: the live arrangement,
    /// carried back verbatim.
    fn workspace_snapshot(&self) -> crate::theme::Workspace {
        crate::theme::Workspace {
            layout: self.layout.clone(),
            project: Some(self.project_path.clone()),
            theme: self.theme_choice.clone(),
            keys: self.raw_keys.clone(),
            shell: self.shell.clone(),
            // Written only when it is not 1: a file that never mentions
            // the zoom means the same as before the line existed.
            zoom: Some(self.zoom).filter(|z| (z - 1.0).abs() > 1e-6),
        }
    }

    /// THE dispatcher: every key press, key press and palette Enter
    /// and palette Enter lands here by action id, so no two doors can
    /// disagree about what an act does. Arrangement acts go through
    /// `Layout::apply`, the layout's own one door.
    fn act(&mut self, id: &str) {
        match id {
            "help" => {
                self.help = !self.help;
                self.overlay_opened_this_frame |= self.help;
            }
            // Esc closes what is on top and nothing beneath it (DESIGN.md
            // §5.4, amended after the review): the palette, else the help,
            // else the unsent drafts, else the creation form — so closing
            // the help never drops a form full of typed values.
            "dismiss" => match view::dismiss_target(self.help, self.palette_open, !self.field_edit.is_empty(), self.new_form.is_some()) {
                view::Dismiss::Palette => self.palette_open = false,
                view::Dismiss::Help => self.help = false,
                view::Dismiss::Drafts => self.field_edit.clear(),
                view::Dismiss::Form => self.new_form = None,
                view::Dismiss::Nothing => {}
            },
            "palette.open" => {
                self.palette_open = true;
                self.palette_query.clear();
                self.palette_sel = 0;
                self.overlay_opened_this_frame = true;
            }
            "cursor.back" => self.set_op_cursor(self.op_cursor.saturating_sub(1)),
            "cursor.on" => self.set_op_cursor(self.op_cursor + 1),
            // The block walk (DESIGN.md §5.6): the arithmetic is
            // `view::block_step`, pure and pinned; the shell applies its
            // pair. Assigned AFTER `set_op_cursor`, whose clearing of the
            // block is the nesting invariant the keys ride over.
            "cursor.block_back" | "cursor.block_on" => {
                if let Ok(g) = &self.gcode {
                    let (k, b) = view::block_step(
                        g,
                        self.part.operations.len(),
                        self.op_cursor,
                        self.block_cursor,
                        id == "cursor.block_on",
                    );
                    self.set_op_cursor(k);
                    self.block_cursor = b;
                    self.scroll_program_to_block = b.is_some();
                }
            }
            // Pure toggles (DESIGN.md I12).
            "sidebar.toggle" => self.layout.apply(DockAction::ToggleSidebar),
            "form.toggle" => self.layout.apply(DockAction::ToggleForm),
            // Git's verbs (DESIGN.md §5.3, Phase 54 slice 3, fork 2A): the
            // three that need no words run at once through the runner's
            // own path; the two that need words put their opening on the
            // dock's line and give it the keyboard.
            "stock.new" => self.offer_line("stock "),
            "git.fetch" => self.start_line("git fetch"),
            "git.pull" => self.start_line("git pull"),
            "git.push" => self.start_line("git push"),
            "git.commit" => self.offer_line("git commit -m "),
            "git.branch" => self.offer_line("git switch "),
            // The window's own frame (Phase 57 slice 1): the caption acts
            // ask the platform through the frame, and the switch toggles
            // the decorations live and remembers the word.
            "window.minimize" => self.pending_viewport.push(egui::ViewportCommand::Minimized(true)),
            "window.maximize" => self.pending_viewport.push(egui::ViewportCommand::Maximized(!self.maximized)),
            "window.close" => self.pending_viewport.push(egui::ViewportCommand::Close),
            "window.frame" => {
                self.layout.frame_native = !self.layout.frame_native;
                self.pending_viewport.push(egui::ViewportCommand::Decorations(self.layout.frame_native));
                if let Err(reason) = crate::theme::save_workspace(&self.workspace_snapshot()) {
                    startup_say(format!("workspace not saved: {reason}"));
                }
            }
            "bottom.toggle" => {
                // The keyboard stays where it was: a dock opened to READ
                // the log must not swallow the single-key acts. An act
                // that puts words on the line (`offer_line`) still takes it.
                self.layout.apply(DockAction::ToggleBottom);
            }
            "output.clear" => self.output.clear(),
            "output.copy" => self.output_copy_pending = true,
            "output.save" => {
                let path = view::log_file_path(std::path::Path::new(&self.project_path));
                let text = self.output.export(&self.project_path, self.opened_unix);
                match crate::theme::write_atomically(&path, &text) {
                    Ok(()) => self.transient = Some((format!("Written — {}: the Output log, {} lines", Self::file_word(&path), self.output.events.len()), std::time::Instant::now())),
                    Err(e) => self.edit_note = Some(format!("Not written — {e}")),
                }
            }
            "output.stop" => self.stop_run(),
            "form.open" => self.layout.apply(DockAction::OpenForm),
            // The target's door (Phase 52 slice 4): the path field opens
            // under the Outline's Target header, which is shown.
            "import.step" => {
                self.target_path_draft = Some(String::new());
                self.layout.apply(DockAction::ShowTab(SidebarTab::Outline));
            }
            "layout.revert" => {
                // The arrangement back to its defaults in one act — and
                // the file is written NOW, not at close, so the act
                // outlives even a crash. The epoch bump gives the panels
                // fresh ids, so the defaults' sizes apply rather than
                // egui's memory.
                self.layout.apply(DockAction::Revert);
                self.panel_epoch += 1;
                if let Err(reason) = crate::theme::save_workspace(&self.workspace_snapshot()) {
                    startup_say(format!("workspace not saved: {reason}"));
                }
            }
            "theme.reload" => {
                // Forget the mtime: the watch reloads on the next frame.
                self.theme_mtime = None;
            }
            "view.fit" => {
                // The first-frame fit again, at whatever size the viewport is now.
                self.fitted = false;
                self.tween = None;
            }
            // The focus (DESIGN.md §5.5, 2026-10-04): answered in the
            // viewport, where the fit has a size to fit to.
            "view.focus" => self.focus_pending = true,
            // The standard views (DESIGN.md §5.5): a stance, the target
            // and the scale kept — `Camera::set_stance`, where the pin is.
            "view.plan" => self.cam.set_stance(view::Stance::Plan),
            "view.front" => self.cam.set_stance(view::Stance::Front),
            "view.right" => self.cam.set_stance(view::Stance::Right),
            // The Program's gutters (DESIGN.md §5.3): toggles through the
            // layout's one door, saved like a layer.
            "program.numbers" => {
                let on = !self.layout.program_numbers;
                self.layout.apply(DockAction::ProgramNumbers(on));
            }
            "program.minimap" => {
                let on = !self.layout.program_minimap;
                self.layout.apply(DockAction::ProgramMinimap(on));
            }
            "project.reload" => {
                self.project_reload = true;
            }
            // The way out (DESIGN.md §5.3, Phase 50 slice 2): the whole
            // program, copied or written beside the project — both refused
            // by name on a rejected program, which has none. Not a write
            // under §1's law: no undo, no reload, the watch unmoved.
            "program.copy" => match &self.gcode {
                Ok(_) => self.program_copy_pending = true,
                Err(_) => self.edit_note = Some("Not copied — this project has errors, so no program was generated".to_string()),
            },
            "program.write" => match &self.gcode {
                Ok(g) => match view::write_program(std::path::Path::new(&self.project_path), &g.text()) {
                    Ok(path) => {
                        let note = view::program_written_note(&Self::file_word(&path), g.lines.len());
                        self.transient = Some((note, std::time::Instant::now()));
                    }
                    Err(e) => self.edit_note = Some(format!("Not written — {e}")),
                },
                Err(_) => self.edit_note = Some("Not written — this project has errors, so no program was generated".to_string()),
            },
            "op.suppress" => self.suppress_selected(),
            "op.move_top" => self.move_selected_op(true),
            "op.move_bottom" => self.move_selected_op(false),
            "statement.delete" => self.delete_selected(),
            "feature.flip_side" => self.flip_selected(),
            "feature.clear_around" => self.clear_around_selected(),
            "tools.renumber" => self.transform_main("renumber tools", |t| view::renumber_tools(t).map(|(text, _)| text)),
            "ops.number" => self.transform_main("number operations", |t| view::number_ops(t, 10)),
            "ops.strip_numbers" => self.transform_main("strip numbers", view::strip_op_numbers),
            "review.accept" => self.accept_selected(),
            "review.reject" => self.reject_selected(),
            "review.accept_all" => self.accept_all(),
            "schedule" => self.schedule_operations(),
            // A creation opens the form's dock (DESIGN.md §5.4): a form
            // composed into a shut dock was live and invisible.
            "feature.new" => {
                // Opens on a pocket, named (DESIGN.md §5.4, Phase 50 slice 5):
                // the persona's first feature, and a name nobody invents.
                let specs = crate::template::FEATURE_SPECS;
                let v = specs.iter().position(|s| s.variant == "ProfilePocket").unwrap_or(0);
                let mut form = NewForm::for_variant(v);
                form.name = crate::template::suggested_name(&self.part, specs[v].variant);
                self.new_form = Some(form);
                self.layout.apply(DockAction::OpenForm);
            }
            // The way in (DESIGN.md §1, Phase 50 slice 3): a main.part from
            // nothing, every value shown before it is written.
            "project.new" => {
                self.new_form = Some(NewForm::for_project(0));
                self.layout.apply(DockAction::OpenForm);
            }
            "tool.new" => {
                self.new_form = Some(NewForm::for_tool(0, crate::template::lowest_undeclared_tool(&self.part)));
                self.layout.apply(DockAction::OpenForm);
            }
            "op.new" => self.new_operation_on_selected(),
            "edit.undo" => self.undo(),
            "edit.redo" => self.redo(),
            // A layer act (DESIGN.md I9): the checkbox's twin.
            _ if Layer::ALL.iter().any(|l| l.action_id() == id) => {
                let l = Layer::ALL.into_iter().find(|l| l.action_id() == id).expect("checked above");
                let on = !self.layout.layers.get(l);
                self.layout.apply(DockAction::Layer(l, on));
            }
            _ if SidebarTab::ALL.iter().any(|t| t.action_id() == id) => {
                if let Some(t) = SidebarTab::ALL.into_iter().find(|t| t.action_id() == id) {
                    self.layout.apply(DockAction::ShowTab(t));
                    // A card the shown tab could not have made is stale
                    // (DESIGN.md §5.4, Phase 51 slice 3): shut; the next
                    // click in that tab reopens the form on its own card.
                    if self.layout.form.open && !view::tab_shows(t, self.selection) {
                        self.layout.apply(DockAction::ShutForm);
                    }
                }
            }
            "center.split" => self.layout.apply(DockAction::ShowSplit),
            _ if Document::ALL.iter().any(|d| d.action_id() == id) => {
                if let Some(d) = Document::ALL.into_iter().find(|d| d.action_id() == id) {
                    self.layout.apply(DockAction::ShowDocument(d));
                }
            }
            _ => {}
        }
    }
    /// One dock: an `egui::Panel` on its wall, holding
    /// one visible pane of several behind a tab strip. A dock with no
    /// panes, or a collapsed one, is not drawn — the viewport takes what
    /// the docks leave. The drawn size is read back into the model as the
    /// `Resize` action, so a drag on the panel edge and a future keyboard
    /// resize land in the same place.
    /// The rail: `view::rail()`, drawn. Every slot's token is a button
    /// through the one dispatcher, its hover says what the slot is, and a
    /// slot reads as ON when it is what you are doing (the live mode) or
    /// what you are looking at (a pane visible in some dock right now).
    /// The shell decides nothing here — which slots exist, their order,
    /// their tokens, their sentences and their acts are all `view`'s, and
    /// `selectable_label` takes its colours from the active theme, so the
    /// strip cannot invent one.
    /// The corner menu (DESIGN.md §5.1, reopened 2026-10-17; Phase 51 slice
    /// 4): the tree is `view::MENU`; a row fires the same act a key does;
    /// a row that is a state wears its check (`view::menu_check`); Window ▸
    /// Theme lists `view::theme_menu`'s rows (slice 5). Drawn at the left
    /// end of the band's first row since the rail retired (Phase 52 slice
    /// 1) — the window's upper-left corner still.
    fn menu_button(&mut self, ui: &mut egui::Ui) {
        let theme_rows = view::theme_menu(
            &crate::theme::SHIPPED_THEMES.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            &crate::theme::folder_themes(),
            self.theme_choice.as_deref(),
        );
        let mut clicked: Option<&'static str> = None;
        let mut theme_pick: Option<Option<String>> = None;
        let layout = self.layout.clone();
        ui.menu_button(view::MENU_GLYPH, |ui| {
            for (title, rows) in view::MENU {
                let mut draw_rows = |ui: &mut egui::Ui| {
                    for (id, label) in rows.iter() {
                        let on = view::menu_check(id, &layout);
                        let button = egui::Button::new(*label).selected(on.unwrap_or(false));
                        let hover = view::action(id).map(|a| view::door_hover(id, a.sentence)).unwrap_or_default();
                        if ui.add(button).on_hover_text(hover).clicked() {
                            clicked = Some(*id);
                            ui.close();
                        }
                    }
                };
                if title.is_empty() {
                    draw_rows(ui);
                    ui.separator();
                } else if *title == "Window" {
                    // Window ▸ Theme (Phase 51 slice 5): the one
                    // submenu whose rows carry a VALUE, as a layer's
                    // checkbox carries a layer — `view::theme_menu`
                    // lists them, `--theme <name>` is the twin.
                    ui.menu_button(*title, |ui| {
                        draw_rows(ui);
                        ui.separator();
                        ui.menu_button("Theme", |ui| {
                            for (label, choice, on) in &theme_rows {
                                if ui.add(egui::Button::new(label.as_str()).selected(*on)).on_hover_text("wear this theme now and at the next launch (nacs --view --theme <name>)").clicked() {
                                    theme_pick = Some(choice.clone());
                                    ui.close();
                                }
                            }
                        });
                    });
                } else {
                    ui.menu_button(*title, |ui| draw_rows(ui));
                }
            }
        })
        .response
        .on_hover_text("the menu — the job's verbs, then File, Edit, View, Window, Help");
        if let Some(id) = clicked {
            self.act(id);
        }
        if let Some(choice) = theme_pick {
            self.pick_theme(choice);
        }
    }

    /// The shut sidebar's door back (I14; DESIGN.md §5.1 as retired
    /// 2026-10-17): one chip beside the menu, `view::SIDEBAR_DOOR`.
    fn sidebar_chip(&mut self, ui: &mut egui::Ui) {
        let (id, label, what) = view::SIDEBAR_DOOR;
        if ui.small_button(label).on_hover_text(view::door_hover(id, what)).clicked() {
            self.act(id);
        }
    }

    /// The emitted program — the same string `nacs`
    /// writes to stdout, from the same formatter, with the operation the
    /// cursor is on picked out. A rejected program shows the error, which
    /// is what the terminal prints in its place.
    ///
    /// The left wall (DESIGN.md §5.2, §5.4): two docks on one wall — the
    /// sidebar's view above, the form beneath — sharing the wall's width.
    /// Either may be shut; the wall is drawn while either is open. The
    /// widths and the form's height read back off the drawn rects as
    /// `Resize` actions, so the model is never behind what is on screen.
    fn left_wall(&mut self, ui: &mut egui::Ui) {
        let sb = self.layout.sidebar.clone();
        let form = self.layout.form.clone();
        if !sb.open && !form.open {
            // Both docks shut: the band's left cell stays (DESIGN.md §5.1
            // as retired 2026-10-17; I14) — the menu and the sidebar's door
            // back. A strip and not a dock, so it costs the file nothing.
            egui::Panel::left("band-cell").resizable(false).frame(band_frame(ui.style())).show(ui, |ui| {
                band_row(ui, |ui| {
                    self.menu_button(ui);
                    self.sidebar_chip(ui);
                });
                band_row(ui, |_ui| {});
            });
            return;
        }
        let id = egui::Id::new(("sidebar", self.panel_epoch));
        // A wall stops where the documents' floors begin (Phase 59 nudge
        // 1): the center keeps a wall's width (`DOCK_MIN_SIZE`) for each
        // document it shows, so a wall dragged wide can no longer push
        // the Program off the window.
        let docs = if matches!(self.layout.center, Center::Split) { 2.0 } else { 1.0 };
        // The center's own margins come off too: without them a wall at
        // its ceiling left the Program 16 px under its floor.
        let chrome = egui::Frame::central_panel(ui.style()).inner_margin.sum().x;
        let room = (ui.available_width() - docs * crate::theme::DOCK_MIN_SIZE - chrome).max(crate::theme::DOCK_MIN_SIZE);
        // The model's floor is the panel's own (nudge 3): egui's default
        // floor for a side panel is 96, the model clamps to 120, and the
        // gap was a spring — a wall dragged under 120 read back as 120.
        let r = egui::Panel::left(id).resizable(true).default_size(sb.width).min_size(crate::theme::DOCK_MIN_SIZE).max_size(room).frame(band_frame(ui.style())).show(ui, |ui| {
            // The band's first row (DESIGN.md §5.1, §5.2 as amended
            // 2026-10-17, Phase 52 slice 1): the menu at the left end, the
            // views as TABS — `view::tabs`, each wearing its count
            // (`view::tab_label`) — and the dock's close at the right. The
            // tabs took the rail's two jobs, so the rail's column and the
            // heading row that repeated its word are gone; so is the hint
            // line ("press ? for help · p for the actions"): each of its
            // two doors stands three times over (the status line, Help ▸,
            // a key).
            let mut clicked: Option<&'static str> = None;
            band_row(ui, |ui| {
                self.menu_button(ui);
                if sb.open {
                    let (n_problems, n_unchecked) = view::problem_counts(&self.rows);
                    let review = match &self.report {
                        Some(Ok(r)) => Some((r.proposed_count(), r.faces_total)),
                        _ => None,
                    };
                    // The tabs take what the close leaves (measured last
                    // frame, Phase 52 slice 12's precedent) inside a
                    // horizontal scroll area with its bar hidden: what does
                    // not fit is clipped at the pane's edge and the row's
                    // rect stays the pane's. Laid plain, a tab strip wider
                    // than the pane widened the pane, so a wall dragged
                    // narrower than its tabs sprang back on release (Phase
                    // 59 nudge 3, the user's walk).
                    let tabs_w = (ui.available_width() - self.sidebar_close_w - ui.spacing().item_spacing.x).max(24.0);
                    let reload_failed = self.reload_failed();
                    ui.scope(|ui| {
                        ui.set_max_width(tabs_w);
                        egui::ScrollArea::horizontal().id_salt("sidebar.tabs").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).auto_shrink([false, false]).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for tab in view::tabs() {
                                    let on = sb.tab == tab.tab;
                                    let label = view::tab_label(&tab, n_problems + usize::from(reload_failed), n_unchecked, review);
                                    if ui.selectable_label(on, label).on_hover_text(tab.sentence).clicked() {
                                        clicked = Some(tab.action);
                                    }
                                }
                            });
                        });
                    });
                } else {
                    self.sidebar_chip(ui);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (id, what) = if sb.open { ("sidebar.toggle", "close the sidebar — the Sidebar chip reopens it") } else { ("form.toggle", "close the form") };
                    let close = ui.small_button("×").on_hover_text(view::door_hover(id, what));
                    self.sidebar_close_w = close.rect.width();
                    if close.clicked() {
                        clicked = Some(id);
                    }
                });
            });
            if let Some(id) = clicked {
                self.act(id);
            }
            // The band's second row (DESIGN.md §5.2, Phase 52 slice 2): the
            // shown view's own line — `view::view_line`, derived; empty,
            // and the row still drawn, while the sidebar is shut and the
            // form has the wall, so the band keeps its height.
            let line = if sb.open {
                let review = match &self.report {
                    Some(Ok(r)) => Some((r.proposed_count(), r.faces_total)),
                    _ => None,
                };
                match sb.tab { SidebarTab::Problems => view::problems_line(&self.rows, self.reload_failed(), &self.account), _ => view::view_line(sb.tab, &self.project_path, self.part.features.len(), self.part.operations.len(), self.op_cursor, &self.rows, self.reload_failed(), review) }
            } else {
                String::new()
            };
            band_row(ui, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(line).weak()).truncate());
            });
            if form.open {
                if sb.open {
                    // The lower dock: the form beneath the tree, with a
                    // floor (`DOCK_MIN_SIZE`) so a long tree scrolls above
                    // a form that stays visible.
                    let fid = egui::Id::new(("form", self.panel_epoch));
                    let fr = egui::Panel::bottom(fid).resizable(true).default_size(form.height).min_size(crate::theme::DOCK_MIN_SIZE).show(ui, |ui| self.form_dock(ui));
                    let drawn = fr.response.rect.height();
                    if (drawn - self.layout.form.height).abs() > 0.5 {
                        self.layout.apply(DockAction::ResizeForm(drawn));
                    }
                } else {
                    // The sidebar shut: the form has the wall.
                    self.form_dock(ui);
                    return;
                }
            }
            // No heading row (Phase 52 slice 1): the tab carries the word
            // and its count; the view's body starts here.
            ui.separator();
            self.job_guide(ui);
            // `auto_shrink` off: egui stores a panel's size as its content's
            // rect, so a scroll area that shrink-wraps its content makes the
            // wall snap to the tree's width every frame (the live run's
            // finding — the tab strip used to hold the width open).
            // A STABLE id for the tree (Phase 51 slice 3): the form's dock is
            // drawn above it in this same panel, so its appearing shifted the
            // tree's auto ids — and a right-click menu, keyed by its row's
            // id, vanished the moment the selection opened the form. An
            // EXPLICIT id, not a salt: egui folds the parent's auto counter
            // into a salted child's id too (`Ui::new_child`), so `push_id`
            // alone still moved.
            ui.scope_builder(egui::UiBuilder::new().id(egui::Id::new(("tree", self.panel_epoch, sb.tab.key()))), |ui| {
                egui::ScrollArea::vertical().id_salt(sb.tab.key()).auto_shrink([false, false]).show(ui, |ui| match sb.tab {
                    SidebarTab::Outline => self.outline_pane(ui),
                    SidebarTab::Problems => self.problems_pane(ui),
                    SidebarTab::Review => self.review_pane(ui),
                });
            });
        });
        let drawn = r.response.rect.width();
        if (drawn - self.layout.sidebar.width).abs() > 0.5 {
            self.layout.apply(DockAction::ResizeSidebar(drawn));
        }
    }

    /// The form's dock (DESIGN.md §5.4): the selection's card, scrolling.
    /// The scroll area takes the dock's whole height, so the dock keeps
    /// the height it was dragged to rather than snapping to the card's.
    fn form_dock(&mut self, ui: &mut egui::Ui) {
        // The dock's header line and its close (DESIGN.md §5.4, Phase 50
        // slice 4): `2` was the only way shut.
        let mut close = false;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("form").small().weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("×").on_hover_text(view::door_hover("form.toggle", "close the form")).clicked() {
                    close = true;
                }
            });
        });
        if close {
            self.act("form.toggle");
        }
        egui::ScrollArea::vertical().id_salt("form").auto_shrink([false, false]).show(ui, |ui| self.card_body(ui));
    }

    /// The bottom dock (DESIGN.md §5.8, I1): *Output* since Phase 54 slice
    /// 1 — the log's lines, oldest first, the newest kept in view; a header
    /// row with the count and *Clear*. Drawn only when opened (`3`).
    fn bottom_panel(&mut self, ui: &mut egui::Ui) {
        let b = self.layout.bottom.clone();
        if !b.open {
            return;
        }
        let id = egui::Id::new(("bottom", self.panel_epoch));
        let mut clear = false;
        let mut stop = false;
        let mut run = false;
        let mut door: Option<&'static str> = None;
        let refusal = rgb(self.theme.refusal);
        let r = egui::Panel::bottom(id).resizable(true).default_size(b.height).min_size(crate::theme::DOCK_MIN_SIZE).show(ui, |ui| {
            band_row(ui, |ui| {
                ui.label(egui::RichText::new("Output").strong());
                let n = self.output.events.len();
                ui.label(egui::RichText::new(format!("{n} {}", if n == 1 { "line" } else { "lines" })).small().weak());
                if ui.small_button("Clear").on_hover_text(view::door_hover("output.clear", "empty the Output log")).clicked() {
                    clear = true;
                }
                if ui.small_button("Copy").on_hover_text(view::door_hover("output.copy", "the whole log to the clipboard")).clicked() {
                    door = Some("output.copy");
                }
                let log_file = view::log_file_path(std::path::Path::new(&self.project_path));
                if ui.small_button("Save").on_hover_text(view::door_hover("output.save", &format!("write the log to {}", log_file.display()))).clicked() {
                    door = Some("output.save");
                }
                // Stop (Phase 59 slice 5): enabled while a run stands.
                if ui.add_enabled(self.runner_child.is_some(), egui::Button::new("Stop").small()).on_hover_text(view::door_hover("output.stop", "stop the run on the line — its process killed")).clicked() {
                    stop = true;
                }
            });
            ui.separator();
            // The runner's line at the dock's bottom (Phase 54 slice 2):
            // Enter runs it; the log above shows what it says. Drawn
            // FIRST, bottom-up, and the log given exactly what remains
            // (Phase 56 slice 1, fork 0A): the log used to be sized by
            // arithmetic that never subtracted the separator under it, so
            // the content stood a few pixels taller than the panel every
            // frame and egui's panel stored the drawn rect as the next
            // frame's size — the crawl Phase 54's walk saw, measured at
            // seven pixels a frame with the line five pixels past the
            // window's edge. Nothing here computes a height, so nothing
            // can get one wrong.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                let hint = if self.runner_rx.is_some() { "running…" } else { "nacs --outline · nacs --rest-report · import <file.step> · git status · sh <line>" };
                // ↑ and ↓ on the focused line walk what was entered
                // (Phase 59 nudge 2; `view::LineHistory` decides): read
                // BEFORE the field is drawn, so the recalled line is what
                // this frame shows, with the cursor put at its end after.
                // The field's event filter keeps the vertical arrows from
                // egui's focus walk, so the line keeps the focus.
                let line_id = egui::Id::new("output.line");
                let mut recalled = false;
                if ui.memory(|m| m.has_focus(line_id)) {
                    let (up, down) = ui.input(|i| (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown)));
                    let hit = if up { self.runner_history.back(&self.runner_line) } else if down { self.runner_history.forward() } else { None };
                    if let Some(l) = hit {
                        self.runner_line = l;
                        recalled = true;
                    }
                }
                let field = ui.add(egui::TextEdit::singleline(&mut self.runner_line).id(line_id).hint_text(hint).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
                if std::mem::take(&mut self.runner_focus_pending) {
                    field.request_focus();
                }
                if recalled {
                    let mut st = egui::TextEdit::load_state(ui.ctx(), line_id).unwrap_or_default();
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(self.runner_line.chars().count()))));
                    egui::TextEdit::store_state(ui.ctx(), line_id, st);
                    // A recalled line is not the refused one.
                    self.runner_refusal = None;
                }
                // A refusal is of the line as it was: edited, it is gone.
                if field.changed() {
                    self.runner_refusal = None;
                }
                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    run = true;
                }
                ui.separator();
                egui::ScrollArea::vertical().id_salt("output.lines").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                    // The lines read top-down inside the log whatever
                    // layout placed the log.
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        for e in &self.output.events {
                            let line = egui::RichText::new(view::OutputLog::render(e)).monospace();
                            ui.label(if e.level == view::Level::Refused { line.color(refusal) } else { line });
                        }
                    });
                });
            });
        });
        if clear {
            self.act("output.clear");
        }
        if let Some(id) = door {
            self.act(id);
        }
        if stop {
            self.act("output.stop");
        }
        if run {
            self.start_run();
        }
        let drawn = r.response.rect.height();
        if (drawn - self.layout.bottom.height).abs() > 0.5 {
            self.layout.apply(DockAction::ResizeBottom(drawn));
        }
    }

    /// The center (DESIGN.md §5.3, amended 2026-09-04): both documents
    /// side by side — the viewport left, the program right, the split
    /// draggable and read back as a `ResizeSplit` like a wall's width —
    /// or one alone. The strip over it is three acts: each document
    /// alone, and both.
    fn center(&mut self, ui: &mut egui::Ui) {
        let center = self.layout.center;
        // The band's first row over the center (DESIGN.md §5.3, Phase 52
        // slice 2): the three acts, once over both documents — and at the
        // right end the project's name and its branch (slice 7), the chip
        // `view::git_chip` composes from what the worker read.
        let (chip, chip_hover) = view::git_chip(&self.project_path, self.git.as_ref());
        let mut git_door: Option<&'static str> = None;
        let mut caption: Option<&'static str> = None;
        let own_frame = !self.layout.frame_native;
        let maximized = self.maximized;
        band_row(ui, |ui| {
            for d in Document::ALL {
                if ui.selectable_label(center == Center::Alone(d), d.title()).clicked() {
                    self.layout.apply(DockAction::ShowDocument(d));
                }
            }
            if ui.selectable_label(center == Center::Split, Center::Split.title()).on_hover_text("the Viewport beside the Program").clicked() {
                self.layout.apply(DockAction::ShowSplit);
            }
            // The chip is a menu (Phase 54 slice 3): git's five verbs, each
            // through the one dispatcher; the hover still says the state.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // The caption buttons (Phase 57 slice 1): outermost first in
                // a right-to-left row — close, maximize or restore, minimize.
                if own_frame {
                    for (id, _, what) in view::CAPTION_DOORS {
                        let glyph = view::caption_glyph(id, maximized);
                        if ui.add(egui::Button::new(glyph).frame(false)).on_hover_text(view::door_hover(id, what)).clicked() {
                            caption = Some(*id);
                        }
                    }
                    ui.separator();
                }
                let r = ui.menu_button(egui::RichText::new(chip).weak(), |ui| {
                    for (id, label) in view::GIT_DOORS {
                        if ui.button(*label).on_hover_text(view::door_hover(id, view::action(id).map_or("", |a| a.sentence))).clicked() {
                            git_door = Some(*id);
                            ui.close();
                        }
                    }
                });
                r.response.on_hover_text(chip_hover);
            });
        });
        if let Some(id) = git_door {
            self.act(id);
        }
        if let Some(id) = caption {
            self.act(id);
        }
        // The second row is each document's own, drawn by the column that
        // holds it, then its body.
        match center {
            Center::Alone(Document::Viewport) => {
                self.viewport_row(ui);
                ui.separator();
                self.viewport(ui);
                // A scroll request the Program is not there to consume is
                // stale by the time it is: dropped, not kept for later.
                self.scroll_program_to_op = None;
                self.scroll_program_to_block = false;
            }
            Center::Alone(Document::Program) => {
                self.program_row(ui);
                ui.separator();
                self.program_document(ui);
            }
            Center::Split => {
                let total = ui.available_width();
                // The documents' floors and the ratio (Phase 59 nudge 1,
                // the user's walk of 2026-10-23): the viewport is a panel
                // and the Program is the width it leaves, so with no
                // ceiling on the panel a wall dragged wide pushed the
                // Program off the window while the viewport kept its
                // pixels — the ratio reached egui only as the panel's
                // first-frame default, and egui's memory held pixels
                // after that. Now the panel's range is [floor, total −
                // floor], the floor a wall's, so both documents keep at
                // least a wall's width; and when the center's width
                // changed by anything but the split's own drag, the panel
                // is re-keyed so the saved ratio sizes it again.
                let floor = crate::theme::DOCK_MIN_SIZE;
                if (total - self.center_total).abs() > 0.5 {
                    self.center_total = total;
                    self.split_epoch += 1;
                }
                let id = egui::Id::new(("center-split", self.panel_epoch, self.split_epoch));
                // Inside the center's own margins already: no top margin,
                // so the viewport's row is level with the Program's.
                let split_frame = egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin { left: 0, right: 8, top: 0, bottom: 0 });
                let r = egui::Panel::left(id).resizable(true).default_size(total * self.layout.split).min_size(floor).max_size((total - floor).max(floor)).frame(split_frame).show(ui, |ui| {
                    self.viewport_row(ui);
                    ui.separator();
                    self.viewport(ui);
                });
                // The ratio is the user's hand on the split and nothing
                // else: read back only while the split's handle is dragged
                // (egui's resize widget, the panel's id with "__resize"),
                // so a floor that clamps the viewport does not rewrite it
                // — a wall's drag used to.
                let dragged = ui.ctx().read_response(id.with("__resize")).is_some_and(|h| h.dragged() || h.drag_stopped());
                let drawn = r.response.rect.width();
                if dragged && total > 0.0 && (drawn / total - self.layout.split).abs() > 0.002 {
                    self.layout.apply(DockAction::ResizeSplit(drawn / total));
                }
                self.program_row(ui);
                ui.separator();
                self.program_document(ui);
            }
        }
    }

    /// The Program's band row (DESIGN.md §5.3 as amended 2026-10-17, Phase
    /// 52 slice 2): the lines label, the two gutters' toggles and the two
    /// ways out on ONE row (two before). The label takes what the controls
    /// leave it and truncates, the whole on hover — measured from LAST
    /// frame's controls (slice 12), the row laid left to right: a
    /// right-to-left row placed its first widget at the right edge and
    /// walked left, past the pane's left edge when the pane was narrower
    /// than the controls (the user's finding after slice 3).
    fn program_row(&mut self, ui: &mut egui::Ui) {
        // The program's size and nothing else: where the cursor stands is
        // the transport bar's to say, and the highlight shows it here.
        let label = match &self.gcode {
            Err(_) => "No program — this project has errors".to_string(),
            Ok(g) => format!("{} lines", g.lines.len()),
        };
        let mut toggle: Option<&'static str> = None;
        let controls_w = self.program_controls_w;
        let mut measured = controls_w;
        band_row(ui, |ui| {
            // The row's width is the pane's, taken BEFORE the scroll area
            // below makes the content's width unbounded.
            let pane_w = ui.available_width();
            // The label keeps at least an ellipsis' width: truncated to
            // "…" it is still the whole sentence on hover, and dropped it
            // is nothing (the band test's finding — at the default window
            // the pane is barely wider than the controls). Narrower than
            // the controls alone, the label goes and the controls stand
            // whole (ROADMAP item 3 slice 6: the first picture of the
            // window found *Write program* clipped at the opening size).
            let room = pane_w - controls_w - ui.spacing().item_spacing.x;
            let label_w = if room < 24.0 { 0.0 } else { room };
            // A horizontal scroll area with its bar hidden: what does not
            // fit is clipped at the pane's right edge, and the row's rect
            // stays the pane's — egui widens a ui to hold what overflows
            // it, and that widening was the spill.
            egui::ScrollArea::horizontal().id_salt("program.row").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).auto_shrink([false, false]).show(ui, |ui| {
                // Centred on the row's height, as `band_row` centres every
                // other row: a scroll area lays its content from the top.
                ui.allocate_ui_with_layout(egui::vec2(pane_w, view::BAND_ROW_H), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    // The label first, in what the controls left last
                    // frame.
                    if label_w > 0.0 {
                        ui.scope(|ui| {
                            ui.set_max_width(label_w);
                            ui.add(egui::Label::new(egui::RichText::new(&label).weak()).truncate()).on_hover_text(&label);
                        });
                    }
                    let controls = ui.scope(|ui| {
                        // The two gutters' toggles (DESIGN.md §5.3, 2026-09-17),
                        // each through the one dispatcher.
                        let mut numbers = self.layout.program_numbers;
                        if ui.checkbox(&mut numbers, "numbers").on_hover_text("Line numbers in the gutter").changed() {
                            toggle = Some("program.numbers");
                        }
                        let mut minimap = self.layout.program_minimap;
                        if ui.checkbox(&mut minimap, "minimap").changed() {
                            toggle = Some("program.minimap");
                        }
                        // The way out (DESIGN.md §5.3, Phase 50 slice 2): the whole
                        // program, to the file beside the project or the clipboard.
                        if ui.button("Copy program").on_hover_text("Copy the whole program to the clipboard.").clicked() {
                            toggle = Some("program.copy");
                        }
                        let file = view::program_file_path(std::path::Path::new(&self.project_path));
                        if ui
                            .button("Write program")
                            .on_hover_text(format!("Write the program to {}.", file.display()))
                            .clicked()
                        {
                            toggle = Some("program.write");
                        }
                    });
                    measured = controls.response.rect.width();
                });
            });
        });
        self.program_controls_w = measured;
        if let Some(id) = toggle {
            self.act(id);
        }
    }

    /// The Viewport's band row (DESIGN.md §5.5 as amended 2026-10-17, Phase
    /// 52 slice 2): the Layers button whose list DROPS DOWN over the picture
    /// — open while hovered, pinned by a click, Phase 51's ruling relocated
    /// and not re-decided (`view::layers_open`) — the legend inline beside
    /// it, and the view strip at the row's right end. One widget still (I9);
    /// the map is the one overlay left (I2); the picture keeps its corners.
    /// Each shown layer's own line, for the Layers list: what the layer
    /// read (the Uncut verdict, the Engagement field's line), or why it
    /// could not be built. These used to stand on the status line for as
    /// long as the layer was on, in front of everything else it had to say.
    fn layer_notes(&self) -> Vec<(Layer, String, bool)> {
        let layers = self.layout.layers;
        let mut out = Vec::new();
        if layers.get(Layer::SimulatedStock) {
            if let Some(Err(reason)) = &self.sim {
                out.push((Layer::SimulatedStock, reason.clone(), true));
            }
        }
        if layers.get(Layer::RestMaterial) {
            match &self.cells {
                Some((_, Ok(c))) => out.push((Layer::RestMaterial, view::rest_verdict(c), false)),
                Some((_, Err(reason))) => out.push((Layer::RestMaterial, reason.clone(), true)),
                None => {}
            }
        }
        if layers.get(Layer::Engagement) {
            out.push(match &self.immersion {
                Some((k, Ok(f))) => {
                    let line = f.line(&self.part);
                    match self.immersion_blocks.as_ref().filter(|(j, _)| j == k).and_then(|(_, b)| view::immersion_drawn_note(b.step)) {
                        Some(drawn) => (Layer::Engagement, format!("{line} · {drawn}"), false),
                        None => (Layer::Engagement, line, false),
                    }
                }
                // A decline, not a failure: the layer has nothing to draw
                // for this selection and says why.
                Some((_, Err(reason))) => (Layer::Engagement, reason.clone(), false),
                None => (Layer::Engagement, "select an operation or a feature with a floor".to_string(), false),
            });
        }
        out
    }

    fn viewport_row(&mut self, ui: &mut egui::Ui) {
        let layers = self.layout.layers;
        let pinned = self.layout.layers_pinned;
        let legend = view::legend(&self.theme);
        let sim_busy = self.sim_rx.is_some();
        // The Uncut layer's verdict (DESIGN.md §5.5, Phase 50 slice 8): the
        // rest report's own sentence, so an empty layer is a verdict and
        // not a blank; nothing while the layer is off or the scan runs.
        let notes = self.layer_notes();
        let refusal = rgb(self.theme.refusal);
        let layers_id = egui::Id::new("layers");
        let list_shown_last_frame = self.layers_list_shown;
        let mut hover_since = self.layers_hover_since;
        let mut changed: Option<(Layer, bool)> = None;
        let mut pin: Option<bool> = None;
        let mut door: Option<&'static str> = None;
        let mut list_shown = false;
        let strip_w = self.viewport_strip_w;
        let mut measured = strip_w;
        band_row(ui, |ui| {
            // The row's width, taken before the scroll area below makes the
            // content's width unbounded; the row inside it laid left to
            // right and centred on its height (Phase 53 slice 3, the
            // Program row's cure): a right-to-left strip walked left past
            // the viewport's edge when the viewport was narrower than its
            // row, and egui widened the ui to hold it.
            let row_w = ui.available_width();
            egui::ScrollArea::horizontal().id_salt("viewport.row").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).auto_shrink([false, false]).show(ui, |ui| {
            ui.allocate_ui_with_layout(egui::vec2(row_w, view::BAND_ROW_H), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            let row_left = ui.max_rect().left();
            let button = ui
                .small_button(if pinned { "Layers  (pinned)" } else { "Layers" })
                .on_hover_text(if pinned { "click to let the layers fold when the pointer leaves" } else { "open while the pointer is here — click to pin it open" });
            if button.clicked() {
                pin = Some(!pinned);
            }
            // The pointer over the list as egui drew it LAST frame — the
            // list hangs under the button, so leaving the button downward
            // keeps it open into the list; a list not shown last frame has
            // only a stale rect, which does not count.
            let over_list = list_shown_last_frame
                && ui.ctx().pointer_hover_pos().is_some_and(|p| ui.ctx().memory(|m| m.area_rect(layers_id)).is_some_and(|r| r.contains(p)));
            // The rest, measured (slice 11): the pointer's arrival is kept
            // while it stays, dropped the frame it leaves; the decision is
            // `view::layers_open`'s. A repaint is asked for the moment the
            // delay runs out, so the list drops with the pointer still. On
            // egui's clock, not `Instant`: the harness steps that clock a
            // quarter second a frame and subtracts the same from every
            // repaint-after, so a wall-clock rest never settles there.
            let now = ui.input(|i| i.time);
            hover_since = if button.hovered() || over_list { Some(hover_since.unwrap_or(now)) } else { None };
            let hovered_for = hover_since.map(|s| now - s);
            let open = view::layers_open(hovered_for, pinned);
            if let (false, Some(t)) = (open, hovered_for) {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64((view::LAYERS_HOVER_DELAY - t).max(0.0)));
            }
            if open {
                list_shown = true;
                egui::Area::new(layers_id)
                    .order(egui::Order::Foreground)
                    .fixed_pos(button.rect.left_bottom())
                    .show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            for l in Layer::ALL {
                                let mut on = layers.get(l);
                                ui.horizontal(|ui| {
                                    if ui.checkbox(&mut on, l.label()).changed() {
                                        changed = Some((l, on));
                                    }
                                    if sim_busy && matches!(l, Layer::SimulatedStock) {
                                        ui.add(egui::Spinner::new().size(10.0));
                                    }
                                });
                                // The layer's own line, under its row: a
                                // readout weak, a failure in the refusal
                                // colour. Wrapped, so a long sentence
                                // makes the list taller and not wider.
                                for (_, text, failed) in notes.iter().filter(|(nl, ..)| *nl == l) {
                                    let text = egui::RichText::new(text).small();
                                    ui.scope(|ui| {
                                        ui.set_max_width(340.0);
                                        ui.add(egui::Label::new(if *failed { text.color(refusal) } else { text.weak() }).wrap());
                                    });
                                }
                            }
                        });
                    });
            }
            // The legend, inline: each role's colour and its word —
            // `view::legend`, the one colour home's own reading.
            for (color, word) in &legend {
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 2.0, rgb(*color));
                ui.label(egui::RichText::new(*word).small());
            }
            // The view strip (DESIGN.md §5.5, Phase 50 slice 4): the
            // camera's five acts, each through the one dispatcher — at the
            // row's right end when it fits, pushed there by the room the
            // button and the legend leave (measured LAST frame), never a
            // right-to-left row.
            let used = ui.cursor().left() - row_left;
            ui.add_space((row_w - used - strip_w - ui.spacing().item_spacing.x).max(0.0));
            let strip = ui.scope(|ui| {
                for (id, label, what) in view::VIEW_STRIP {
                    if ui.small_button(*label).on_hover_text(view::door_hover(id, what)).clicked() {
                        door = Some(*id);
                    }
                }
            });
            measured = strip.response.rect.width();
            });
            });
        });
        self.layers_list_shown = list_shown;
        self.layers_hover_since = hover_since;
        self.viewport_strip_w = measured;
        if let Some((l, on)) = changed {
            self.layout.apply(DockAction::Layer(l, on));
        }
        if let Some(on) = pin {
            self.layout.apply(DockAction::LayersPinned(on));
        }
        if let Some(id) = door {
            self.act(id);
        }
    }

    /// The Program document: the same string `nacs` writes to stdout, from
    /// the same formatter, the cursor's operation picked out. Monospace and
    /// NOT wrapped — a move read half a line at a time is a different move —
    /// at the center's full width, virtualized (`show_rows`).
    fn program_document(&mut self, ui: &mut egui::Ui) {
        let g = match &self.gcode {
            Ok(g) => g,
            Err(error) => {
                ui.label(
                    egui::RichText::new("This project has errors — no program was generated")
                        .color(rgb(self.theme.refusal)),
                );
                ui.label(egui::RichText::new(error).color(rgb(self.theme.text)));
                return;
            }
        };
        let here = g.at_cursor(self.op_cursor);
        let (numbers_on, minimap_on) = (self.layout.program_numbers, self.layout.program_minimap);
        // `show_rows` lays out only the rows on screen. A program is the
        // one pane whose content runs to thousands of lines, and drawing
        // them all to show forty would put the whole file in every frame.
        let row = ui.text_style_height(&egui::TextStyle::Monospace);
        let (on_text, on_back, k) =
            (rgb(self.theme.text_open), rgb(self.theme.element_selected), self.op_cursor);
        // The active block's line (DESIGN.md §5.3): the move→line
        // correspondence read off the view model, marked in the pick's own
        // colours inside the op highlight.
        let active_block_line = match (here, self.block_cursor) {
            (Some(op), Some(b)) => g.block_line(op, b),
            _ => None,
        };
        let (pick_text, pick_back) = (rgb(self.theme.ground), rgb(self.theme.select));
        // Scroll only on a block KEY: the offset centres the target row,
        // because `show_rows` lays out only what is visible, so a row off
        // screen has no widget to scroll to.
        // `auto_shrink` off in both axes (DESIGN.md §5.3, Phase 52 slice 3,
        // the user's "big thing"): egui's default shrinks the area to its
        // longest line, so the vertical scrollbar hugged the text while the
        // minimap strip stood at the pane's edge, and the air between read
        // as a divider the minimap does not have. The sidebar met the same
        // fact on 2026-09-04. The document fills its pane to the strip.
        // SOLID scroll bars (Phase 52 slice 15, the user's nudge): egui's
        // floating bars are thin overlays that fade, and a mouse without
        // a side wheel had no visible way along a line longer than the
        // pane. The document wore the solid style alone until Phase 59
        // slice 2 made it the window's one style, installed with the
        // visuals in `ui()`; nothing here sets one. The row above keeps
        // its hidden bar by design.
        let mut area = egui::ScrollArea::both().id_salt("gcode.lines").auto_shrink([false, false]);
        // One offset per frame, at the pitch `show_rows` lays rows at (the
        // row height PLUS the spacing — `view::program_scroll_offset`, the
        // review's finding: the old arithmetic landed 85 rows short at
        // line 500). An Outline click brings the operation's first line
        // into view (DESIGN.md §5.3, amendment 2026-09-04) — the reverse of
        // the program's own click, which selects — and wins over a block
        // key's request when both stand.
        let spacing = ui.spacing().item_spacing.y;
        let avail = ui.available_height();
        let mut target: Option<f32> = None;
        if self.scroll_program_to_block {
            if let Some(line) = active_block_line {
                target = Some(view::program_scroll_offset(row, spacing, line, avail));
            }
            self.scroll_program_to_block = false;
        }
        if let Some(op) = self.scroll_program_to_op.take() {
            if let Some(span) = g.span(op) {
                target = Some(view::program_scroll_offset(row, spacing, span.start, avail));
            }
        }
        if let Some(t) = target {
            area = area.vertical_scroll_offset(t);
        }
        // The minimap's strip takes the right edge (DESIGN.md §5.3); a
        // click or drag on it becomes a scroll offset for THIS frame's
        // area, through `view::minimap_scroll_to`.
        const STRIP_W: f32 = MINIMAP_STRIP_W;
        // The pane as the document finds it, for the harness (slice 3).
        let pane_rect = ui.available_rect_before_wrap();
        let strip_rect = if minimap_on {
            let full = ui.available_rect_before_wrap();
            let strip = egui::Rect::from_min_max(egui::pos2(full.right() - STRIP_W, full.top()), full.right_bottom());
            let resp = ui
                .interact(strip, ui.id().with("program.minimap"), egui::Sense::click_and_drag())
                .on_hover_text("the whole program, one row per line in its operation's colour — click or drag to scroll there (the strip's width is fixed)");
            if resp.clicked() || resp.dragged() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    area = area.vertical_scroll_offset(view::minimap_scroll_to(pos.y - strip.top(), strip.height(), g.lines.len(), row, spacing, avail));
                }
            }
            Some(strip)
        } else {
            None
        };
        let width = view::line_number_width(g.lines.len());
        let muted = rgb(self.theme.text);
        // A click seeks (DESIGN.md §5.3): collected here, applied after the
        // view model's borrow ends.
        let mut clicked: Option<usize> = None;
        let mut visible: std::ops::Range<usize> = 0..0;
        let out = area.show_rows(
            ui,
            row,
            g.lines.len(),
            |ui, rows| {
                visible = rows.clone();
                // The strip's width is kept clear of the text.
                if strip_rect.is_some() {
                    ui.set_max_width(ui.available_width() - STRIP_W);
                }
                for (i, line) in g.lines[rows.clone()].iter().enumerate() {
                    let abs = rows.start + i;
                    let mut t = egui::RichText::new(&line.text).monospace();
                    // The cursor's own op is what faces you; the ops still
                    // to run are dimmed, exactly as the outline dims them.
                    // The active block wears the pick's colours within it.
                    // The highlight is the ROW's width, painted behind it
                    // (the review: a glyph-width highlight), so the text
                    // carries only its colour.
                    let back = if Some(abs) == active_block_line {
                        t = t.color(pick_text);
                        Some(pick_back)
                    } else if g.lit(abs, k) {
                        t = t.color(on_text);
                        Some(on_back)
                    } else {
                        if line.op.is_some_and(|op| op >= k) {
                            t = t.weak();
                        }
                        None
                    };
                    let row_rect = ui.available_rect_before_wrap();
                    let row_rect = egui::Rect::from_min_size(row_rect.min, egui::vec2(row_rect.width(), row));
                    if let Some(b) = back {
                        ui.painter().rect_filled(row_rect, 0.0, b);
                    }
                    let resp = ui.horizontal(|ui| {
                        if numbers_on {
                            ui.label(egui::RichText::new(format!("{:>width$}", abs + 1)).monospace().color(muted));
                        }
                        // Every line seeks (Phase 52 slice 14), and says so
                        // under the pointer.
                        ui.add(egui::Label::new(t).sense(egui::Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand)
                    }).inner;
                    if resp.clicked() {
                        clicked = Some(abs);
                    }
                }
            },
        );
        // The minimap, drawn last over the strip: one slice per line in
        // its op's role colour (`view::minimap_row`), the cursor's op lit,
        // the visible span framed — nothing the document does not say.
        if let Some(strip) = strip_rect {
            let p = ui.painter().with_clip_rect(strip);
            p.rect_filled(strip, 0.0, rgba(self.theme.map_ground, self.theme.fill_alpha.map_ground));
            let n = g.lines.len();
            for (i, line) in g.lines.iter().enumerate() {
                let Some(op) = line.op else { continue };
                let (top, bottom) = view::minimap_row(i, n, strip.height());
                let role = self.part.operations.get(op).map(crate::parse::derive_op_role);
                let color = match role {
                    Some(r) => {
                        let (c, a) = view::feed_paint(&self.theme, r, Some(op) == here, false);
                        rgba(c, a)
                    }
                    None => muted,
                };
                let r = egui::Rect::from_min_max(egui::pos2(strip.left() + 4.0, strip.top() + top), egui::pos2(strip.right() - 4.0, strip.top() + bottom.max(top + 1.0)));
                p.rect_filled(r, 0.0, color);
            }
            let (t0, _) = view::minimap_row(visible.start, n, strip.height());
            let (_, b1) = view::minimap_row(visible.end.saturating_sub(1), n, strip.height());
            let frame = egui::Rect::from_min_max(egui::pos2(strip.left(), strip.top() + t0), egui::pos2(strip.right(), strip.top() + b1.max(t0 + 2.0)));
            p.rect_stroke(frame, 0.0, egui::Stroke::new(1.0, rgb(self.theme.border_hover)), egui::StrokeKind::Inside);
        }
        self.program_rects = Some((pane_rect, out.inner_rect));
        // The click's answer, read off the view model (`seek`, Phase 52
        // slice 14): a header line is before operation 1, a trailer line
        // after the last, an op's line after that op at the move it is.
        let seek = clicked.and_then(|i| g.seek(i).map(|s| (s, g.place(i))));
        if let Some(((k, block), place)) = seek {
            // The cursor, then the move within it; the order matters —
            // `set_op_cursor` clears the block by invariant.
            self.set_op_cursor(k);
            self.block_cursor = block;
            // And the operation is the selection (DESIGN.md §5.3, widened
            // 2026-08-29): the inspector and the outline answer the click —
            // and since 2026-09-17 the sidebar brings the row into view
            // (the reverse of the reverse). The header and the trailer
            // select nothing: they are nobody's.
            if let Some(view::Place::Op(op)) = place {
                self.set_selection(view::Selection::Operation(op));
                self.scroll_sidebar_to_op = Some(op);
            }
        }
    }

    /// The Outline (DESIGN.md §5.2 as amended 2026-10-17, Phase 52 slice 4):
    /// five groups, top to bottom — Stock · Target · Features · Operations ·
    /// Tools — the sections `--outline` prints, each a collapsible header
    /// carrying its door on the header row; then what the file holds back.
    /// Every row is the same `ProjectOutline` the `--outline` report renders,
    /// each op line built by the same formatter (`outline::op_line`), so pane
    /// and report cannot drift. The run order was the Sequence view until it
    /// folded in here: one home, the same `OpNode`s as the tree's.
    fn outline_pane(&mut self, ui: &mut egui::Ui) {
        let mut act: Option<&'static str> = None;
        // A Program click's request: the row brought into view, once — read
        // here for both the tree and the run order.
        let follow = self.scroll_sidebar_to_op.take();
        // 4c: every project has a stock (`project.new` writes one), so the
        // header has no door while one stands — and the model always holds
        // one, so it never shows.
        // The Stock header's door (Phase 54 slice 5b): the noun exists now
        // (slice 5a), so the header keeps the door Phase 52 fork 4c held
        // back until it did.
        self.outline_group(
            ui,
            "Stock",
            Some(("stock.new", "New stock…", "Propose the blank from a STEP file. z undoes.")),
            &mut act,
            |s, ui, act| s.outline_stock(ui, act),
        );
        self.outline_group(
            ui,
            "Target",
            Some(("import.step", "New target…", "Import a STEP file as the model being made, by path or by dropping it on the window.")),
            &mut act,
            |s, ui, act| s.outline_target(ui, act),
        );
        self.outline_group(
            ui,
            "Features",
            Some(("feature.new", "New feature…", "Add a feature. z undoes.")),
            &mut act,
            |s, ui, act| s.outline_features(ui, act, follow),
        );
        let feature_selected = view::selected_feature(self.selection, &self.rows, &self.edges).is_some();
        self.outline_group(
            ui,
            "Operations",
            Some(("op.new", "New operation…", if feature_selected { "Add an operation on the selected feature. z undoes." } else { "Select a feature first — an operation is composed on one" })),
            &mut act,
            |s, ui, act| s.outline_operations(ui, act, follow),
        );
        self.outline_group(
            ui,
            "Tools",
            Some(("tool.new", "New tool…", "Add a tool. z undoes.")),
            &mut act,
            |s, ui, act| s.outline_tools(ui, act),
        );
        if let Some(id) = act {
            if id == "op.new" && !feature_selected {
                // The door is drawn disabled; a key or the palette still
                // reaches the act, which refuses on its own.
            }
            self.act(id);
        }

        // What the file holds back (DESIGN.md §5.2, Suppressed): its own
        // heading, because the groups above list the program and this
        // lists the file. Absent when empty.
        if !self.suppressed.is_empty() {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Suppressed").strong());
            let mut restore: Option<(usize, usize)> = None;
            for s in &self.suppressed {
                ui.horizontal(|ui| {
                    if ui.small_button("Restore").on_hover_text("Put this operation back in the program. z undoes.").clicked() {
                        restore = Some((s.file, s.line));
                    }
                    ui.label(egui::RichText::new(&s.text).weak());
                });
            }
            if let Some((file, line)) = restore {
                self.restore(file, line);
            }
        }
    }

    /// One group of the Outline: a collapsing header, open by default,
    /// carrying its door at the row's right end — `(act, label, hover)`, the
    /// act fired through the one dispatcher — then the group's rows.
    fn outline_group(
        &mut self,
        ui: &mut egui::Ui,
        title: &'static str,
        door: Option<(&'static str, &'static str, &'static str)>,
        act: &mut Option<&'static str>,
        body: impl FnOnce(&mut Self, &mut egui::Ui, &mut Option<&'static str>),
    ) {
        let id = ui.make_persistent_id(("outline-group", title));
        let enabled = match door {
            Some(("op.new", _, _)) => view::selected_feature(self.selection, &self.rows, &self.edges).is_some(),
            _ => true,
        };
        let header = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true).show_header(ui, |ui| {
            ui.label(egui::RichText::new(title).strong());
            if let Some((aid, label, hover)) = door {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Truncated to what the title leaves (nudge 3): laid
                    // plain, the door widened the pane under a narrow drag.
                    if ui.add_enabled(enabled, egui::Button::new(label).small().truncate()).on_hover_text(hover).clicked() {
                        *act = Some(aid);
                    }
                });
            }
        });
        header.body(|ui| body(self, ui, act));
    }

    /// The stock's row — `outline::stock_row`, the words `--outline` prints —
    /// selects the stock (DESIGN.md §5.2, §6; Phase 22 slice 1), so its
    /// height is raised from the form like a tool's holder.
    fn outline_stock(&mut self, ui: &mut egui::Ui, act: &mut Option<&'static str>) {
        let checked = self.selection == view::Selection::Stock;
        let resp = ui.selectable_label(checked, outline::stock_row(&self.outline.stock));
        if resp.clicked() {
            self.set_selection(if checked { view::Selection::None } else { view::Selection::Stock });
        }
        let (mut pick, mut menu) = (None, None);
        row_menu(&resp, view::Selection::Stock, &mut pick, &mut menu);
        if let Some(sel) = pick {
            self.set_selection(sel);
        }
        if let Some(id) = menu {
            *act = Some(id);
        }
    }

    /// The target (DESIGN.md §5.2, Phase 52 slice 4): the model being made
    /// — the report's source and totals (`view::target_row`), a click
    /// showing the Review; no report, where one comes from; and the path
    /// field while *New target…* has opened it — Enter or *Import* starts
    /// the sidecar as a drop does, `×` or Esc-less `×` closes it.
    fn outline_target(&mut self, ui: &mut egui::Ui, act: &mut Option<&'static str>) {
        match &self.report {
            Some(Ok(r)) => {
                let row = view::target_row(&r.source, r.faces_total, r.proposed_count());
                if ui.selectable_label(false, row).on_hover_text("the model the features are recognized from — click for the Review, face by face").clicked() {
                    *act = Some(SidebarTab::Review.action_id());
                }
            }
            Some(Err(e)) => {
                ui.label(egui::RichText::new(format!("report.json: {e}")).color(rgb(self.theme.refusal)));
            }
            None => {
                ui.label(egui::RichText::new(view::NO_TARGET).weak());
            }
        }
        let (mut go, mut shut, mut browse) = (false, false, false);
        let dialog_open = self.dialog_rx.is_some();
        if let Some(draft) = self.target_path_draft.as_mut() {
            ui.horizontal(|ui| {
                // The native dialog (slice 9), where the platform has one;
                // the field stands beside it, and alone elsewhere.
                if HAS_FILE_DIALOG && ui.add_enabled(!dialog_open, egui::Button::new("Browse…").small()).on_hover_text("Choose the STEP file in the system's open dialog").clicked() {
                    browse = true;
                }
                let field = ui.add(egui::TextEdit::singleline(draft).hint_text("path to a .step or .stp file").desired_width((ui.available_width() - 96.0).max(80.0)));
                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    go = true;
                }
                if ui.small_button("Import").on_hover_text("Run the import on this file (nacs --import <file> <project>)").clicked() {
                    go = true;
                }
                if ui.small_button("×").on_hover_text("close the field — nothing imported").clicked() {
                    shut = true;
                }
            });
            // The setup under the path (ROADMAP item 1, slice 4): the
            // words `nacs --import` takes, kept for the next import too.
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("up").small().weak()).on_hover_text("which MODEL axis points up on the machine; z+ is as modeled (nacs --import --up)");
                let shown = self.import_setup.up.clone().unwrap_or_else(|| "z+".to_string());
                egui::ComboBox::from_id_salt("import up").selected_text(shown.clone()).width(56.0).show_ui(ui, |ui| {
                    for axis in view::UP_AXES {
                        if ui.selectable_label(shown == axis, axis).clicked() {
                            self.import_setup.up = (axis != "z+").then(|| axis.to_string());
                        }
                    }
                });
                ui.label(egui::RichText::new("solid").small().weak()).on_hover_text("which solid is the part when the file holds several — S1, S2…; empty for a one-solid file (nacs --import --solid)");
                ui.add(egui::TextEdit::singleline(&mut self.import_setup.solid).hint_text("one").desired_width(36.0));
                ui.label(egui::RichText::new("allowance").small().weak()).on_hover_text("how far the blank stands above the model's top and outside its plan, in mm: the proposed stock grows by it, and the top face and the rim become passes; 0 for a blank cut to size (nacs --import --allowance)");
                ui.add(egui::TextEdit::singleline(&mut self.import_setup.allowance).hint_text("0").desired_width(36.0));
                ui.label(egui::RichText::new("given").small().weak()).on_hover_text("the program before this one, for a second setup: its project as the given from line will name it from this folder (../front.cam); the proposed stock is that program's blank turned over and the import writes the given from line beside it (nacs --import --given)");
                ui.add(egui::TextEdit::singleline(&mut self.import_setup.given).hint_text("none").desired_width(110.0));
            });
        }
        if go {
            let path = std::path::PathBuf::from(self.target_path_draft.take().unwrap_or_default().trim());
            if view::is_step(&path) && path.is_file() {
                self.start_import(path);
            } else {
                self.edit_note = Some(format!("Not imported — {} is not a STEP file that exists", path.display()));
            }
        }
        if shut {
            self.target_path_draft = None;
        }
        if browse && self.dialog_rx.is_none() {
            // On a worker: the dialog blocks its thread until it closes.
            let given = std::path::PathBuf::from(&self.project_path);
            let dir = if given.is_dir() { given } else { given.parent().map(|p| p.to_path_buf()).unwrap_or_default() };
            let (tx, rx) = std::sync::mpsc::channel();
            self.dialog_rx = Some(rx);
            std::thread::spawn(move || {
                let _ = tx.send(open_step_dialog(&dir));
            });
        }
    }

    /// The features: the tree as it stands — each feature's operations
    /// nested beneath it, the removal lines beside them — then the three
    /// lists as sub-headings: what shapes an operation and is scheduled by
    /// none, what is declared and not scheduled (with *New operation…* on
    /// the row), and what is given.
    fn outline_features(&mut self, ui: &mut egui::Ui, act: &mut Option<&'static str>, follow: Option<usize>) {
        let mut choose: Option<view::Selection> = None;
        // A feature row selects the feature (DESIGN.md §5.2, §6) — by its
        // index among the drawn edges, which is what the viewport's pick
        // uses too, so the two doors agree. Before 2026-09-04 a feature
        // could be reached only by clicking its wire.
        let feature_sel = |edges: &[FeatureEdges], name: &str| edges.iter().position(|e| e.name == name).map(view::Selection::Feature);
        for node in &self.outline.features {
            let sel = feature_sel(&self.edges, &node.name);
            let checked = sel.is_some() && Some(self.selection) == sel;
            let resp = ui.selectable_label(checked, format!("{}  {}", node.name, node.kind));
            if resp.clicked() {
                choose = Some(if checked { view::Selection::None } else { sel.unwrap_or(view::Selection::None) });
            }
            if let Some(s) = sel {
                row_menu(&resp, s, &mut choose, act);
            }
            ui.indent(("feature-ops", &node.name), |ui| {
                for op in &node.ops {
                    let line = outline::op_line(&self.part, op);
                    let checked = self.selection == view::Selection::Operation(op.idx);
                    let past = op.idx < self.op_cursor;
                    let text = if past { egui::RichText::new(line) } else { egui::RichText::new(line).weak() };
                    // ⏶⏷ live in the Operations group (the Sequence until
                    // Phase 52 slice 4): a reorder is a run-order act, and
                    // it has one home.
                    let resp = ui.selectable_label(checked, text);
                    if follow == Some(op.idx) {
                        resp.scroll_to_me(Some(egui::Align::Center));
                    }
                    if resp.clicked() {
                        choose = Some(if checked { view::Selection::None } else { view::Selection::Operation(op.idx) });
                    }
                    row_menu(&resp, view::Selection::Operation(op.idx), &mut choose, act);
                    // The stock model's own line for the op — `--outline`'s.
                    ui.label(egui::RichText::new(self.outline.removals.get(op.idx).map_or("", |s| s.as_str())).small().weak());
                    // The air a body can stand in, per given void (Phase 42
                    // slice 2) — the same lines the terminal prints.
                    for line in self.outline.given_air.get(op.idx).into_iter().flatten() {
                        ui.label(egui::RichText::new(line.as_str()).small().weak());
                    }
                }
            });
        }

        // A feature declared and not scheduled is one operation away, so
        // the door is on its row (DESIGN.md §5.2, amendment 2026-09-04 after
        // the live run): the row selects the feature and opens the ninth
        // write's form on it.
        let mut schedule: Option<view::Selection> = None;
        // Two lists where there was one (Phase 37 slice 5): a feature an
        // operation goes around or is walled by is used, and says how; only
        // the second list is "usually a mistake". Both rows keep the door.
        let shaping = self.outline.shaping.iter().map(|n| ("Shapes an operation, scheduled by none:", n.name.clone(), crate::outline::shaping_row(n)));
        let unused = self.outline.unused.iter().map(|n| ("Declared but not scheduled:", n.name.clone(), format!("{}  {}", n.name, n.kind)));
        // The third list (Phase 42 slice 1): what is given is air before
        // the program starts — the same row shape, its own heading.
        let given = self.outline.given.iter().map(|n| ("Given — air before this program starts:", n.name.clone(), format!("{}  {}", n.name, n.kind)));
        let unscheduled: Vec<(&str, String, String)> = shaping.chain(unused).chain(given).collect();
        let mut said: Option<&str> = None;
        for (heading, name, row) in &unscheduled {
            if said != Some(*heading) {
                ui.label(egui::RichText::new(*heading).small());
                said = Some(*heading);
            }
            let sel = feature_sel(&self.edges, name);
            let checked = sel.is_some() && Some(self.selection) == sel;
            ui.horizontal(|ui| {
                if ui.selectable_label(checked, format!("  {row}")).clicked() {
                    choose = Some(if checked { view::Selection::None } else { sel.unwrap_or(view::Selection::None) });
                }
                if let Some(sel) = sel {
                    if ui.small_button("New operation…").on_hover_text("Add an operation on this feature. z undoes.").clicked() {
                        schedule = Some(sel);
                    }
                }
            });
        }
        if let Some(sel) = schedule {
            self.set_selection(sel);
            *act = Some("op.new");
        }
        if let Some(sel) = choose {
            self.set_selection(sel);
            // The program follows an Outline click onto an operation.
            if let view::Selection::Operation(i) = sel {
                self.scroll_program_to_op = Some(i);
            }
        }
    }

    /// The operations in run order (DESIGN.md §5.2 — the Sequence view from
    /// 2026-09-04, folded into the Outline 2026-10-17): `outline::sequence`,
    /// each row the same `op_line` as the tree's (a visit to several holes
    /// is filed under each in the tree and is one row here: one operation,
    /// run once), the cursor drawn as the line between what has run and
    /// what will, ⏶⏷ on every row (the fourth write: a reorder is a
    /// run-order act, one home); a row selects the operation and brings
    /// its line into view in the Program, as the tree's click does.
    fn outline_operations(&mut self, ui: &mut egui::Ui, act: &mut Option<&'static str>, follow: Option<usize>) {
        let ops = outline::sequence(&self.outline);
        let n = ops.len();
        let k = self.op_cursor.min(n);
        if n == 0 {
            ui.label(egui::RichText::new("No operations — a feature's New operation… adds one").weak());
            return;
        }
        let mut choose: Option<view::Selection> = None;
        let mut reorder: Option<(usize, bool)> = None;
        let select = rgb(self.theme.select);
        let divider = |ui: &mut egui::Ui| {
            // A RULE, not words (Phase 52 slice 13, fork 13b): the cursor is
            // the line between what has run and what will, drawn in the
            // select colour across the row; the count small beside it, a
            // node the harness can find. No arrow glyph: the font has none
            // (the live run drew a box).
            // The counts read in the rule's own direction (slice 16, the
            // user's nudge): what has run stands ABOVE the rule, so its
            // count points up at the left — "1 ⏶"; what is left stands
            // below, so its count points down at the right — "1 ⏷". The
            // right label is measured first so the rule takes exactly what
            // the two leave it (never a right-to-left row: slice 12).
            ui.horizontal(|ui| {
                let font = egui::TextStyle::Small.resolve(ui.style());
                let weak = ui.visuals().weak_text_color();
                let left = format!("{k} ⏶");
                let right = format!("{} ⏷", n - k);
                let right_w = ui.painter().layout_no_wrap(right.clone(), font.clone(), weak).size().x;
                ui.label(egui::RichText::new(left).small().weak()).on_hover_text(format!("{k} run — above the line"));
                let rule_w = (ui.available_width() - right_w - ui.spacing().item_spacing.x).max(8.0);
                let (r, resp) = ui.allocate_exact_size(egui::vec2(rule_w, ui.spacing().interact_size.y * 0.5), egui::Sense::hover());
                ui.painter().hline(r.x_range(), r.center().y, egui::Stroke::new(2.0, select));
                resp.on_hover_text("the cursor — the line between what has run and what will  ([ ], the slider, a click in the Program)");
                ui.label(egui::RichText::new(right).small().weak()).on_hover_text(format!("{} to run — below the line", n - k));
            });
        };
        for (i, op) in ops.iter().enumerate() {
            if i == k {
                divider(ui);
            }
            let line = outline::op_line(&self.part, op);
            let checked = self.selection == view::Selection::Operation(op.idx);
            let text = if op.idx < k { egui::RichText::new(line.as_str()) } else { egui::RichText::new(line.as_str()).weak() };
            ui.horizontal(|ui| {
                if ui.small_button("⏶").on_hover_text("Run this operation earlier. z undoes.").clicked() {
                    reorder = Some((op.idx, false));
                }
                if ui.small_button("⏷").on_hover_text("Run this operation later. z undoes.").clicked() {
                    reorder = Some((op.idx, true));
                }
                let resp = ui.add(egui::Button::selectable(checked, text).truncate()).on_hover_text(&line);
                if follow == Some(op.idx) {
                    resp.scroll_to_me(Some(egui::Align::Center));
                }
                if resp.clicked() {
                    choose = Some(if checked { view::Selection::None } else { view::Selection::Operation(op.idx) });
                }
                // The third door (DESIGN.md §5.2, Phase 50 slice 9): a
                // right-click selects the row and offers its acts; a
                // double-click focuses it.
                row_menu(&resp, view::Selection::Operation(op.idx), &mut choose, act);
            });
        }
        if k == n {
            divider(ui);
        }
        if let Some(sel) = choose {
            self.set_selection(sel);
            if let view::Selection::Operation(i) = sel {
                self.scroll_program_to_op = Some(i);
            }
        }
        if let Some((op, down)) = reorder {
            self.reorder(op, down);
        }
    }

    /// The declarations (DESIGN.md §5.2, Tools): every tool the program
    /// declares, in number order, each row `outline::tool_row`'s — the
    /// same words `--outline` prints. A row selects the tool (§6).
    fn outline_tools(&mut self, ui: &mut egui::Ui, act: &mut Option<&'static str>) {
        let mut pick: Option<view::Selection> = None;
        if self.outline.tools.is_empty() {
            ui.label(egui::RichText::new("No tools — New tool… declares one").weak());
        }
        for node in &self.outline.tools {
            let checked = self.selection == view::Selection::Tool(node.num);
            let text = outline::tool_row(node);
            let text = if node.ops.is_empty() { egui::RichText::new(text).weak() } else { egui::RichText::new(text) };
            let resp = ui.selectable_label(checked, text);
            if resp.clicked() {
                pick = Some(if checked { view::Selection::None } else { view::Selection::Tool(node.num) });
            }
            row_menu(&resp, view::Selection::Tool(node.num), &mut pick, act);
        }
        if let Some(sel) = pick {
            self.set_selection(sel);
        }
    }

    /// The dialog's answer (Phase 52 slice 9): a path imports as a drop
    /// does; a cancel closes nothing — the field stays for a path by hand.
    fn poll_dialog(&mut self) {
        let Some(rx) = &self.dialog_rx else { return };
        match rx.try_recv() {
            Ok(Some(path)) => {
                self.dialog_rx = None;
                self.target_path_draft = None;
                self.start_import(path);
            }
            Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => self.dialog_rx = None,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    /// The repository's head (DESIGN.md §5.3, Phase 52 slice 7): `git`
    /// run on a worker every few seconds — a process is not frame work —
    /// its answer the band's chip. Read-only; the verbs are Phase 53's.
    fn poll_git(&mut self) {
        const EVERY: std::time::Duration = std::time::Duration::from_secs(3);
        if let Some(rx) = &self.git_rx {
            match rx.try_recv() {
                Ok(head) => {
                    self.git = Some(head);
                    self.git_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.git_rx = None,
            }
            return;
        }
        if self.git_read_at.is_some_and(|t| t.elapsed() < EVERY) {
            return;
        }
        self.git_read_at = Some(std::time::Instant::now());
        let given = std::path::PathBuf::from(&self.project_path);
        let dir = if given.is_dir() { given } else { given.parent().map(|p| p.to_path_buf()).unwrap_or_default() };
        let (tx, rx) = std::sync::mpsc::channel();
        self.git_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(view::git_head(&dir));
        });
    }

    /// The review's watch (DESIGN.md §5.2): the report beside the project,
    /// re-read when its mtime moves — including into or out of existence.
    fn poll_report(&mut self) {
        let mtime = std::fs::metadata(&self.report_path).and_then(|m| m.modified()).ok();
        let first = self.report.is_none() && self.report_mtime.is_none() && self.review_rows.is_empty();
        if mtime == self.report_mtime && !first {
            return;
        }
        self.report_mtime = mtime;
        self.report = match std::fs::read_to_string(&self.report_path) {
            Ok(text) => Some(crate::review::read(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => Some(Err(format!("cannot read {}: {e}", self.report_path.display()))),
        };
        self.review_drafts.clear();
        self.refresh_review_rows();
    }

    /// The proposed stock's watch: `proposed_stock.part` beside the report,
    /// re-read when its mtime moves — into or out of existence.
    fn poll_stock_proposal(&mut self) {
        let path = self.stock_proposal_path();
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if mtime == self.stock_mtime && (mtime.is_some() || self.stock_text.is_none()) {
            return;
        }
        self.stock_mtime = mtime;
        self.stock_text = std::fs::read_to_string(&path).ok();
        self.refresh_review_rows();
    }

    fn stock_proposal_path(&self) -> std::path::PathBuf {
        self.report_path.with_file_name("proposed_stock.part")
    }

    /// After a run that may have proposed: read the files now, not on the
    /// next tick, and put the Review in front with the stock row selected
    /// when there is one — the next click is Accept.
    fn show_proposals(&mut self) {
        self.report_mtime = None;
        self.stock_mtime = None;
        self.poll_report();
        self.poll_stock_proposal();
        self.layout.apply(DockAction::ShowTab(SidebarTab::Review));
        if matches!(self.stock_proposal, Some(Ok(ref s)) if !s.in_project) {
            self.set_selection(view::Selection::ProposedStock);
        } else if self.review_rows.iter().any(|r| r.statement.is_some()) {
            self.set_selection(view::Selection::Review(0));
        }
    }

    /// The rows follow both the report and the program: `in_project` is
    /// read off the program's feature names, so an accept — by this window
    /// or any editor — shows after the reload it causes.
    fn refresh_review_rows(&mut self) {
        // The rejections are lines of main.part, read off the text as the
        // acceptances are read off the program.
        let rejected = self.part.sources.first().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| crate::review::rejected_ids(&t)).unwrap_or_default();
        self.review_rows = match &self.report {
            Some(Ok(r)) => crate::review::rows(r, &self.part, &rejected),
            _ => Vec::new(),
        };
        self.review_dups = view::duplicate_proposals(&self.review_rows);
        self.stock_proposal = self.stock_text.as_deref().map(|text| view::stock_proposal(text, &self.part));
        if let view::Selection::Review(i) = self.selection {
            if i >= self.review_rows.len() {
                self.selection = view::Selection::None;
            }
        }
        if self.selection == view::Selection::ProposedStock && !matches!(self.stock_proposal, Some(Ok(_))) {
            self.selection = view::Selection::None;
        }
        self.proposal_edges = self.lit_proposal();
    }

    /// The selected proposal's edges, derived from the selection (DESIGN.md
    /// §6: everything else derives) — computed at the selection's one
    /// writer and at every refresh, not per frame.
    fn lit_proposal(&self) -> Option<Vec<FeatureEdges>> {
        let view::Selection::Review(i) = self.selection else { return None };
        let statement = self.review_rows.get(i)?.statement.as_deref()?;
        view::proposal_edges(statement).ok()
    }

    /// The second write (DESIGN.md §1, §5.4): the selected proposal's
    /// statement appended to `main.part`, checked through the one parser
    /// first, through the same atomic write, undo stack and reload as
    /// suppress. The report is never written.
    fn accept_selected(&mut self) {
        if self.selection == view::Selection::ProposedStock {
            self.accept_proposed_stock();
            return;
        }
        let view::Selection::Review(i) = self.selection else {
            self.edit_note = Some("Not written — select a proposal to accept".to_string());
            return;
        };
        let Some(row) = self.review_rows.get(i) else { return };
        let Some(statement) = row.statement.clone() else {
            self.edit_note = Some(format!("Not written — {} was declined by the report; there is nothing to accept", row.id));
            return;
        };
        // The card's edit, when there is one, is what gets written.
        let edited = self.review_drafts.get(&row.id).filter(|d| d.trim() != statement.trim()).cloned();
        let what = if edited.is_some() { format!("{} accepted as edited", row.id) } else { format!("{} accepted", row.id) };
        let statement = edited.unwrap_or(statement);
        let rejected_as: Vec<String> = if row.rejected { self.review_rows.iter().filter(|r| r.statement == row.statement).map(|r| r.id.clone()).collect() } else { Vec::new() };
        if row.in_project {
            self.edit_note = Some("Not written — that feature is already in the project".to_string());
            return;
        }
        // The same question create faced, the same answer (DESIGN.md §1,
        // amendment 2026-09-03): with the feature declarations in
        // geometry.part, an append to main.part lands beside a different
        // home, and the window does not pick files for the author.
        if view::second_file_declares_features(&self.part) {
            self.edit_note = Some(
                "Not written — geometry.part carries the feature declarations, so the destination is ambiguous; declare it there by hand".to_string(),
            );
            return;
        }
        let Some(path) = self.part.sources.first().cloned() else {
            self.edit_note = Some("Not written — this project has no file on disk to append to".to_string());
            return;
        };
        let result = crate::review::check_statement(&statement)
            .and_then(|()| std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display())))
            .and_then(|before| {
                // Accepting a rejected proposal takes its rejection out in
                // the same write: the text never says both.
                let ids: Vec<&str> = rejected_as.iter().map(String::as_str).collect();
                let cleared = if ids.is_empty() { before.clone() } else { crate::review::without_rejection(&before, &ids).unwrap_or_else(|_| before.clone()) };
                let after = view::append_statement(&cleared, &statement);
                crate::theme::write_atomically(&path, &after)?;
                Ok((before, after))
            });
        match result {
            Ok((before, after)) => {
                let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                self.note_written(&path, &what);
                self.edits.record(view::Edit { path, before, after, stamp });
                self.edit_note = None;
            }
            Err(e) => self.edit_note = Some(format!("Not written — {e}")),
        }
    }

    /// Reject the selected proposal (ROADMAP item 1, slice 3): one comment
    /// line, `# rejected F7`, appended to main.part, so the row says so and
    /// Accept all skips it — a fact of the project, read back off the text
    /// by this window, by `--review`, and by anyone's editor. On a rejected
    /// row the same act takes the line out again. z undoes either.
    fn reject_selected(&mut self) {
        let view::Selection::Review(i) = self.selection else {
            self.edit_note = Some("Not written — select a proposal to reject".to_string());
            return;
        };
        let Some(row) = self.review_rows.get(i).cloned() else { return };
        if row.statement.is_none() {
            self.edit_note = Some(format!("Not written — {} was declined by the report already; there is nothing to reject", row.id));
            return;
        }
        if row.in_project {
            self.edit_note = Some("Not written — that feature is in the project; delete it from the Outline instead".to_string());
            return;
        }
        let Some(path) = self.part.sources.first().cloned() else {
            self.edit_note = Some("Not written — this project has no file on disk to write to; accept the stock first".to_string());
            return;
        };
        // The group's members and any row proposing the same statement are
        // rejected together, as they are accepted together.
        let same: Vec<String> = self.review_rows.iter().filter(|r| r.statement == row.statement).map(|r| r.id.clone()).collect();
        let what = if row.rejected { format!("{} rejection taken back", row.id) } else { format!("{} rejected", row.id) };
        let result = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display())).and_then(|before| {
            let after = if row.rejected {
                let ids: Vec<&str> = same.iter().map(String::as_str).collect();
                crate::review::without_rejection(&before, &ids)?
            } else {
                crate::review::with_rejection(&before, &row.id)
            };
            crate::theme::write_atomically(&path, &after)?;
            Ok((before, after))
        });
        match result {
            Ok((before, after)) => {
                let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                self.note_written(&path, &what);
                self.edits.record(view::Edit { path, before, after, stamp });
                self.edit_note = None;
            }
            Err(e) => self.edit_note = Some(format!("Not written — {e}")),
        }
    }

    /// The run lines for every feature no run names yet (ROADMAP item 1,
    /// slice 10), appended to main.part in one write: `schedule::for_project`
    /// reads the project's own files and judges every line with the parser,
    /// so what lands always parses; the notes land as comments.
    fn schedule_operations(&mut self) {
        let Some(path) = self.part.sources.first().cloned() else {
            self.edit_note = Some("Not written — this project has no file on disk to append to".to_string());
            return;
        };
        let result = crate::schedule::for_project(&self.part).and_then(|s| {
            let text = crate::schedule::render(&s);
            if s.lines.is_empty() {
                return Err(match s.notes.len() {
                    0 => "nothing to schedule: every declared feature is named by a run".to_string(),
                    _ => format!("nothing to schedule: {}", s.notes.join("; ")),
                });
            }
            let before = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let after = view::append_statement(&before, &text);
            self.reparse_with(0, &after)?;
            crate::theme::write_atomically(&path, &after)?;
            Ok((before, after, s.lines.len(), s.notes.len()))
        });
        match result {
            Ok((before, after, n, notes)) => {
                let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                let said = if notes == 0 { format!("{n} operations scheduled") } else { format!("{n} operations scheduled, {notes} notes") };
                self.note_written(&path, &said);
                self.edits.record(view::Edit { path, before, after, stamp });
                self.edit_note = None;
            }
            Err(e) => self.edit_note = Some(format!("Not written — {e}")),
        }
    }

    /// Every proposal at once (DESIGN.md §5.2, Phase 50 slice 10): the
    /// statements not yet in the project, the duplicates skipped, each
    /// checked as accept checks one, the whole project re-parsed with them
    /// all appended, then ONE write — the second write's path, once.
    fn accept_all(&mut self) {
        // The rejected rows stay out; a row's edit on the card goes in.
        let statements: Vec<String> = self
            .review_rows
            .iter()
            .enumerate()
            .filter(|(i, r)| !r.in_project && !r.rejected && self.review_dups.get(*i).copied().flatten().is_none())
            .filter_map(|(_, r)| r.statement.as_ref().map(|s| self.review_drafts.get(&r.id).filter(|d| d.trim() != s.trim()).cloned().unwrap_or_else(|| s.clone())))
            .collect();
        if statements.is_empty() {
            self.edit_note = Some("Not written — nothing to accept: every proposal is in the project, rejected, or repeats one".to_string());
            return;
        }
        if view::second_file_declares_features(&self.part) {
            self.edit_note = Some("Not written — geometry.part carries the feature declarations, so the destination is ambiguous; declare them there by hand".to_string());
            return;
        }
        let Some(path) = self.part.sources.first().cloned() else {
            self.edit_note = Some("Not written — this project has no file on disk to append to".to_string());
            return;
        };
        let n = statements.len();
        let result = statements
            .iter()
            .try_for_each(|s| crate::review::check_statement(s))
            .and_then(|()| std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display())))
            .and_then(|before| {
                let mut after = before.clone();
                for s in &statements {
                    after = view::append_statement(&after, s);
                }
                self.reparse_with(0, &after)?;
                crate::theme::write_atomically(&path, &after)?;
                Ok((before, after))
            });
        match result {
            Ok((before, after)) => {
                let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                self.note_written(&path, &format!("{n} proposals accepted"));
                self.edits.record(view::Edit { path, before, after, stamp });
                self.edit_note = None;
            }
            Err(e) => self.edit_note = Some(format!("Not written — {e}")),
        }
    }

    /// The runner's Enter (DESIGN.md §5.8, Phase 54 slice 2, fork 1A): the
    /// line judged by `view::runner_command` — refused by name on the
    /// status line, nothing run — then the process spawned on a worker in
    /// the project's directory, stdout and stderr read line by line into
    /// the log under the echoed command, the exit said last; one run at a
    /// time. Never a shell.
    /// The proposed stock accepted (Phase 54 slice 5b): `proposed_stock.part`
    /// beside main.part — the sidecar's one block — put in place of the
    /// standing stock statement (`view::replace_stock_statement`) through
    /// the read, the one parser in memory, the atomic write, the undo
    /// record and the watch's reload the other writes use; refused by name
    /// on the status line when the file is not there or the project would
    /// not parse with it.
    fn accept_proposed_stock(&mut self) {
        let Some(path) = self.part.sources.first().cloned() else {
            self.edit_note = Some("Not written — this project has no main.part on disk".to_string());
            return;
        };
        let proposal_path = path.parent().map(|d| d.join("proposed_stock.part")).unwrap_or_default();
        // A project not yet made (Phase 56 slice 4, fork 4A): the window
        // stands on the empty stance, no main.part on disk — the proposal
        // MAKES it, the template's header with the model's stock in place
        // of the template's, through the same parse-then-write as New
        // project. Phase 54's walk ran *New stock…* on a directory that did
        // not exist and found no path at all.
        if !path.exists() {
            let dir = path.parent().map(|d| d.to_path_buf()).unwrap_or_default();
            let made = std::fs::read_to_string(&proposal_path)
                .map_err(|e| format!("cannot read {}: {e}", proposal_path.display()))
                .and_then(|proposal| {
                    let text = crate::template::project_text_from_proposal(&proposal)?;
                    // Parsed where it will live: a `given from` line names
                    // the earlier project from this folder (slice 12).
                    crate::parse::parse_project_with_base(&text, None, Some(&dir))?;
                    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
                    crate::theme::write_atomically(&path, &text)?;
                    Ok(proposal_points(&proposal))
                });
            match made {
                Ok(n) => {
                    self.transient = Some((format!("Written — main.part made from the model: a stock Profile of {n} points and the header's defaults"), std::time::Instant::now()));
                    self.edit_note = None;
                    self.project_reload = true;
                    // The sidebar stays on the Review: the proposed
                    // features are the next thing to accept.
                }
                Err(e) => self.edit_note = Some(format!("Not written — {e}")),
            }
            return;
        }
        let Some((file, line, _)) = self.part.stock_line.clone() else {
            self.edit_note = Some("Not written — this project declares no stock line to replace".to_string());
            return;
        };
        if file != 0 {
            self.edit_note = Some("Not written — the stock statement is not in main.part".to_string());
            return;
        }
        let result = std::fs::read_to_string(&proposal_path)
            .map_err(|e| format!("cannot read {}: {e}", proposal_path.display()))
            .and_then(|proposal| {
                let before = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
                let after = view::replace_stock_statement(&before, line, &proposal)?;
                self.reparse_with(0, &after)?;
                crate::theme::write_atomically(&path, &after)?;
                Ok((before, after, proposal_points(&proposal)))
            });
        match result {
            Ok((before, after, n)) => {
                let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                self.note_written(&path, &format!("stock Profile of {n} points from the model, in place of the stock line"));
                self.edits.record(view::Edit { path, before, after, stamp });
                self.edit_note = None;
            }
            Err(e) => self.edit_note = Some(format!("Not written — {e}")),
        }
    }

    /// A line run as if typed and entered (the band's git verbs).
    fn start_line(&mut self, line: &str) {
        if !self.layout.bottom.open {
            self.layout.apply(DockAction::ToggleBottom);
        }
        self.runner_line = line.to_string();
        self.start_run();
    }

    /// A line's opening put on the dock's line with the keyboard — the
    /// words are the user's to finish (a commit's message, a branch's name).
    fn offer_line(&mut self, opening: &str) {
        if !self.layout.bottom.open {
            self.layout.apply(DockAction::ToggleBottom);
        }
        self.runner_line = opening.to_string();
        self.runner_focus_pending = true;
    }

    fn start_run(&mut self) {
        let line = std::mem::take(&mut self.runner_line);
        // Remembered whatever comes of it (Phase 59 nudge 2): a refused
        // line is the one most worth bringing back.
        self.runner_history.remember(&line);
        // A new Enter supersedes the last refusal, whatever this one says.
        self.runner_refusal = None;
        if self.runner_rx.is_some() {
            self.runner_refusal = Some("Not run — a run is still going".to_string());
            self.runner_line = line;
            return;
        }
        let given = std::path::Path::new(&self.project_path);
        let dir = if given.extension().is_some_and(|e| e == "part") { given.parent().map(|p| p.to_path_buf()).unwrap_or_default() } else { given.to_path_buf() };
        // Absolute, because the run's working directory IS the project
        // directory and a relative path handed to the process would be
        // read against that (found by the pin: `plate.cam/plate.cam`).
        let dir = view::absolute(&dir);
        let nacs = match view::nacs_exe() {
            Ok(p) => p,
            Err(e) => {
                self.runner_refusal = Some(format!("Not run — {e}"));
                return;
            }
        };
        // Absolute, as `dir` is: the process runs in the project directory,
        // and a relative `sidecar/` would be read against that (found by
        // slice 4's pin — the runner's `stock` had never run the real
        // sidecar in a test, and the window's own cwd is the repo root).
        let sidecar = view::sidecar_dir().map(|s| view::absolute(&s));
        let spec = match view::runner_command(&line, &dir, &nacs, sidecar.as_deref(), self.shell.as_deref()) {
            Ok(Some(s)) => s,
            Ok(None) => return,
            Err(e) => {
                self.runner_refusal = Some(format!("Not run — {e}"));
                return;
            }
        };
        // A stock run on a project not yet made (Phase 56 slice 4): the
        // directory is made first, as `--init` makes it — the sidecar
        // writes its proposal there and the process runs there.
        if spec.echo.starts_with("stock ") && !dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                self.runner_refusal = Some(format!("Not run — create {}: {e}", dir.display()));
                self.runner_line = line;
                return;
            }
        }
        let t = self.opened_at.elapsed().as_secs_f64();
        self.output.event(t, view::Level::Info, "run", &format!("$ {}", spec.echo));
        if let Some(via) = &spec.via {
            self.output.append(t, via);
        }
        match spawn_run(&spec) {
            Ok((child, rx)) => {
                self.runner_busy = Some(spec.echo.clone());
                self.runner_child = Some(child);
                self.runner_killed = false;
                self.runner_rx = Some(rx);
            }
            // A program that cannot start is one line, and the line is free.
            Err(e) => self.output.append(t, &e),
        }
    }

    /// Stop (Phase 59 slice 5, fork 5A): the run's process killed, its
    /// exit line "killed" when the frame reads it. The whole TREE on
    /// Windows — `cmd /C` runs the line as a child of its own, and
    /// TerminateProcess on cmd alone leaves that child running with the
    /// pipes in hand, so the run would never end; `taskkill /T` takes the
    /// tree, `/F` because a process with no window has no close message
    /// to answer. Elsewhere `kill` is SIGKILL on the shell, which `sh -c`
    /// made the command itself. For every verb, not only `sh`: a long
    /// `--patterns` or a sidecar import can be stopped too.
    fn stop_run(&mut self) {
        let Some(child) = self.runner_child.as_mut() else { return };
        self.runner_killed = true;
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("taskkill").args(["/T", "/F", "/PID", &child.id().to_string()]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
        }
        // The whole group on Unix (`spawn_run` gave it its own): the
        // negative id names the group.
        #[cfg(unix)]
        {
            let _ = std::process::Command::new("kill").args(["-KILL", "--", &format!("-{}", child.id())]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
        }
        let _ = child.kill();
    }

    /// A STEP dropped on the window (Phase 50 slice 10): the sidecar run
    /// on a worker into this window's project directory, judged before it
    /// starts; one import at a time.
    fn start_import(&mut self, step: std::path::PathBuf) {
        if self.import_rx.is_some() {
            self.edit_note = Some("Not imported — an import is already running".to_string());
            return;
        }
        let Some(sidecar) = view::sidecar_dir() else {
            self.edit_note = Some("Not imported — no sidecar in reach: run the window from the repo root, or set NACS_SIDECAR to the sidecar directory".to_string());
            return;
        };
        let given = std::path::Path::new(&self.project_path);
        let dir = if given.extension().is_some_and(|e| e == "part") { given.parent().map(|p| p.to_path_buf()).unwrap_or_default() } else { given.to_path_buf() };
        let setup = self.import_setup.clone();
        if let Err(e) = view::import_command(&sidecar, &step, &dir, &setup) {
            self.edit_note = Some(format!("Not imported — {e}"));
            return;
        }
        self.import_busy = Some(step.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
        self.import_last = Some(step.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        self.import_rx = Some(rx);
        std::thread::spawn(move || {
            // The blank after the features: the model's own outline for a
            // prism, the smallest blank it fits otherwise. Err is the
            // sidecar failing, which is not an import failure.
            let result = view::run_import(&sidecar, &step, &dir, &setup).map(|summary| (summary, view::run_stock(&sidecar, &step, &dir, &setup)));
            let _ = tx.send(result);
        });
    }

    /// The watch (DESIGN.md §1): when the newest mtime over the project's
    /// files moves — or the palette asked — the project is re-read. A file
    /// that fails to load changes nothing on screen: the last good program
    /// stays, the refusal stands on the status line until a later reload
    /// succeeds. The observed mtime is kept either way, so a broken file is
    /// read once per change, not once per frame.
    fn poll_project(&mut self, ctx: &egui::Context) {
        // A reload runs on a worker (ROADMAP item 3 slice 2): the load and
        // the check are the program's whole planning, seconds under a fine
        // stepover, and until 2026-11-07 they ran here, on the frame, so the
        // window read as crashed. While one is in flight the status line
        // says so and nothing else is reloaded; a file that changes
        // meanwhile is seen by the poll after the adopt.
        if let Some((rx, switched, spawned_on)) = &self.reload_rx {
            let (switched, spawned_on) = (*switched, *spawned_on);
            match rx.try_recv() {
                Ok(Ok((part, cr))) => {
                    self.reload_rx = None;
                    self.adopt(part, cr);
                    self.project_mtime = spawned_on;
                }
                Ok(Err(reason)) => {
                    self.reload_rx = None;
                    if switched {
                        // A project just switched to that does not load opens
                        // the way `--view` opens one (DESIGN.md §1, Phase 50
                        // slice 3): the empty stance, the reason its Problems
                        // row — never "Not reloaded" over the old project.
                        let part = view::empty_project_part(std::path::Path::new(&self.project_path));
                        let cr = CheckResult { refusal: Some(reason), findings: Vec::new(), emitted_moves: None };
                        self.adopt(part, cr);
                        self.project_mtime = spawned_on;
                    } else {
                        self.project_note = Some(format!("Not reloaded — {reason}"));
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.reload_rx = None;
                    self.project_note = Some("Not reloaded — the reload worker died".to_string());
                }
            }
            return;
        }
        let mtime = view::newest_mtime(&self.part.sources);
        if !self.project_reload && mtime == self.project_mtime {
            return;
        }
        self.project_reload = false;
        self.project_mtime = mtime;
        let switched = std::mem::take(&mut self.project_switch);
        let path = self.project_path.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let loaded = crate::parse::load_project(&path)
                .map(|part| {
                    let cr = crate::check_result::check_result(&part);
                    (part, cr)
                })
                .map_err(|e| match e {
                    crate::parse::LoadError::Io(s) | crate::parse::LoadError::Parse(s) => s,
                });
            let _ = tx.send(loaded);
        });
        self.reload_rx = Some((rx, switched, mtime));
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }

    /// Open another project in this window (DESIGN.md §1, Phase 50 slice
    /// 3): the path becomes the one the watch follows and the title names,
    /// the report path moves with it, the undo stack (a stack of THIS
    /// file's texts) and any form are dropped, the workspace remembers it,
    /// and the next poll loads it — or lands it in a stance.
    fn switch_project(&mut self, project: std::path::PathBuf, ctx: &egui::Context) {
        let path = project.to_string_lossy().to_string();
        self.project_path = path.clone();
        self.report_path = (if project.extension().is_some_and(|e| e == "part") { project.parent().map(|d| d.to_path_buf()).unwrap_or_default() } else { project.clone() }).join("report.json");
        self.stock_mtime = None;
        self.stock_text = None;
        self.stock_proposal = None;
        self.stock_declined = None;
        self.review_drafts.clear();
        self.edits = view::EditHistory::default();
        self.new_form = None;
        self.field_edit.clear();
        self.edit_note = None;
        self.project_switch = true;
        self.project_reload = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(path));
        if let Err(reason) = crate::theme::save_workspace(&self.workspace_snapshot()) {
            startup_say(format!("workspace not saved: {reason}"));
        }
    }

    /// The file a parsed operation's `run` line is in — `Part::op_lines`
    /// names the source by index, `Part::sources` the path. A synthesized
    /// operation, or a project parsed from text, has none.
    fn op_source(&self, op: usize) -> Option<(std::path::PathBuf, usize)> {
        let (file, line) = *self.part.op_lines.get(op)?;
        let path = self.part.sources.get(file)?.clone();
        Some((path, line))
    }

    /// The one write (DESIGN.md §1): the selected operation's `run` line
    /// gains its `#`, the whole file goes through the atomic write, the
    /// edit is recorded for undo, and the watch reloads on the next frame
    /// — the same reload any editor's save gets. Anything that cannot be
    /// done changes nothing and says so on the status line.
    fn suppress_selected(&mut self) {
        let view::Selection::Operation(op) = self.selection else {
            self.edit_note = Some("Not written — select an operation to suppress".to_string());
            return;
        };
        let Some((path, line)) = self.op_source(op) else {
            self.edit_note = Some("Not written — this operation has no run line on disk".to_string());
            return;
        };
        let result = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))
            .and_then(|before| view::suppress_line(&before, line).map(|after| (before, after)))
            .and_then(|(before, after)| {
                crate::theme::write_atomically(&path, &after)?;
                Ok((before, after))
            });
        match result {
            Ok((before, after)) => {
                let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                self.note_written(&path, &format!("operation [{}] suppressed", op));
                self.edits.record(view::Edit { path, before, after, stamp });
                self.edit_note = None;
            }
            Err(e) => self.edit_note = Some(format!("Not written — {e}")),
        }
    }

    /// The tenth write (DESIGN.md §1): the selected statement — an
    /// operation, a feature, a tool — removed from its file, its line or
    /// its block (`view::statement_extent`, `view::delete_lines`), and
    /// nothing else. The whole project is re-parsed in memory with the
    /// lines gone before anything is written, so a feature something runs
    /// or a tool something runs is refused with the PARSER's sentence —
    /// never a pre-check that names the operations, which would be a
    /// second judge. `z` undoes it, which is the confirmation. The stock
    /// is not deleted (a project has one); a problem or a review row is
    /// not a statement of the project.
    fn delete_selected(&mut self) {
        let (found, what) = match self.selection {
            view::Selection::Operation(i) => (self.part.op_lines.get(i).copied(), format!("operation [{}] deleted", i)),
            view::Selection::Feature(i) => match self.edges.get(i).map(|e| e.name.clone()) {
                Some(name) => (self.part.feature_lines.iter().find(|(n, ..)| *n == name).map(|(_, f, l, _)| (*f, *l)), format!("feature {name} deleted")),
                None => (None, String::new()),
            },
            view::Selection::Tool(t) => (self.part.tool_lines.iter().find(|(n, ..)| *n == t).map(|(_, f, l, _)| (*f, *l)), format!("tool T{t} deleted")),
            view::Selection::Stock => {
                self.edit_note = Some("Not written — the stock is not deleted: a project has one; change its line instead".to_string());
                return;
            }
            _ => {
                self.edit_note = Some("Not written — select an operation, a feature or a tool to delete".to_string());
                return;
            }
        };
        let Some((file, line)) = found else {
            self.edit_note = Some("Not written — that statement has no line on disk".to_string());
            return;
        };
        let Some(path) = self.part.sources.get(file).cloned() else {
            self.edit_note = Some("Not written — that file is not on disk".to_string());
            return;
        };
        let result = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))
            .and_then(|before| {
                let (first, last) = view::statement_extent(&before, line)?;
                let after = view::delete_lines(&before, first, last)?;
                self.reparse_with(file, &after)?;
                crate::theme::write_atomically(&path, &after)?;
                Ok((before, after))
            });
        match result {
            Ok((before, after)) => {
                let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                self.note_written(&path, &what);
                self.edits.record(view::Edit { path, before, after, stamp });
                self.edit_note = None;
                // The statement is gone; an index that now names its
                // neighbour, under the button that just deleted, is a trap.
                self.set_selection(view::Selection::None);
            }
            Err(e) => self.edit_note = Some(format!("Not written — {e}")),
        }
    }

    /// The loop's side flipped (Phase 53 slice 2, fork 1A): the selected
    /// feature's block rewritten by `view::flip_side` — the kind word
    /// swapped, the points traversed the other way — through the same
    /// read, parse-in-memory, atomic write, undo record and reload as the
    /// delete; the selection stays (the feature keeps its name). A kind
    /// without a side, or no feature selected, is said and nothing is
    /// written.
    /// *Clear around it…* (Phase 61 slice 3): the Plane at the selected
    /// boss's base, opened as the feature form with its name and z filled —
    /// a proposal in the form, nothing written until Create (tenet 2: smart
    /// from context, never silently committed). When it lands, the
    /// operation form opens on it with the Plane's skeletons.
    fn clear_around_selected(&mut self) {
        let view::Selection::Feature(i) = self.selection else {
            self.edit_note = Some("Not written — select a boss to clear around".to_string());
            return;
        };
        let Some(name) = self.edges.get(i).map(|e| e.name.clone()) else { return };
        match view::clear_around_plane(&self.part, &name) {
            Ok((plane, z)) => {
                let specs = crate::template::FEATURE_SPECS;
                let Some(v) = specs.iter().position(|s| s.variant == "Plane") else { return };
                let mut form = NewForm::for_variant(v);
                form.name = plane.clone();
                if let Some(k) = specs[v].keys.iter().position(|k| k.name == "z") {
                    form.values[k] = crate::gcode::f(z).trim_end_matches('0').trim_end_matches('.').to_string();
                }
                self.new_form = Some(form);
                self.edit_note = None;
                self.layout.apply(DockAction::OpenForm);
            }
            Err(e) => self.edit_note = Some(format!("Not written — {e}")),
        }
    }

    fn flip_selected(&mut self) {
        let view::Selection::Feature(i) = self.selection else {
            self.edit_note = Some("Not written — select a feature to flip its side".to_string());
            return;
        };
        let Some(name) = self.edges.get(i).map(|e| e.name.clone()) else {
            self.edit_note = Some("Not written — that feature has no line on disk".to_string());
            return;
        };
        let Some((file, line)) = self.part.feature_lines.iter().find(|(n, ..)| *n == name).map(|(_, f, l, _)| (*f, *l)) else {
            self.edit_note = Some("Not written — that statement has no line on disk".to_string());
            return;
        };
        let Some(path) = self.part.sources.get(file).cloned() else {
            self.edit_note = Some("Not written — that file is not on disk".to_string());
            return;
        };
        let result = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))
            .and_then(|before| {
                let after = view::flip_side(&before, line)?;
                self.reparse_with(file, &after)?;
                crate::theme::write_atomically(&path, &after)?;
                Ok((before, after))
            });
        match result {
            Ok((before, after)) => {
                let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                let kind = self.edges.get(i).map(|e| e.kind.to_string()).unwrap_or_default();
                let to = view::flipped_kind(&kind).unwrap_or("the other side");
                self.note_written(&path, &format!("feature {name} flipped to {to}"));
                self.edits.record(view::Edit { path, before, after, stamp });
                self.edit_note = None;
            }
            Err(e) => self.edit_note = Some(format!("Not written — {e}")),
        }
    }

    /// The one parser, in memory, on the whole project as it would be
    /// with `after` in place of file `file`'s text (the sixth, seventh,
    /// ninth and tenth writes' check). geometry.part by NAME: the source
    /// list carries the parse's externals after the part files, so on a
    /// project with no geometry.part the second source is a machine file
    /// or a meridian — handed to the parser as part text, it would refuse
    /// every write on that project (found while building the seventh
    /// write, which reads the same seam).
    fn reparse_with(&self, file: usize, after: &str) -> Result<(), String> {
        let (main, geom) = if file == 0 {
            let geom_src = self
                .part
                .sources
                .get(1)
                .filter(|g| g.file_name().is_some_and(|n| n == "geometry.part"))
                .and_then(|g| std::fs::read_to_string(g).ok());
            (after.to_string(), geom_src)
        } else {
            (std::fs::read_to_string(&self.part.sources[0]).map_err(|e| e.to_string())?, Some(after.to_string()))
        };
        let base = std::path::Path::new(&self.project_path);
        let base = if base.is_dir() { Some(base) } else { base.parent() };
        crate::parse::parse_project_with_base(&main, geom.as_deref(), base).map(|_| ())
    }

    /// Undo: the text the last write replaced goes back, if the file has
    /// not moved since that write. Refused by name otherwise — a stale
    /// undo that clobbers someone else's save is the one thing worse than
    /// no undo.
    fn undo(&mut self) {
        let Some(e) = self.edits.to_undo() else {
            self.edit_note = Some("Nothing to undo".to_string());
            return;
        };
        let (path, text) = (e.path.clone(), e.before.clone());
        match view::rewrite_if_unmoved(&path, &text, &self.edits) {
            Ok(new_stamp) => {
                self.edits.undone(new_stamp);
                self.edit_note = None;
                self.transient = Some((view::undone_note(&Self::file_word(&path), false), std::time::Instant::now()));
            }
            Err(reason) => self.edit_note = Some(format!("Not undone — {reason}")),
        }
    }

    fn redo(&mut self) {
        let Some(e) = self.edits.to_redo() else {
            self.edit_note = Some("Nothing to redo".to_string());
            return;
        };
        let (path, text) = (e.path.clone(), e.after.clone());
        match view::rewrite_if_unmoved(&path, &text, &self.edits) {
            Ok(new_stamp) => {
                self.edits.redone(new_stamp);
                self.edit_note = None;
                self.transient = Some((view::undone_note(&Self::file_word(&path), true), std::time::Instant::now()));
            }
            Err(reason) => self.edit_note = Some(format!("Not redone — {reason}")),
        }
    }

    /// The file's own name, for a note.
    fn file_word(path: &std::path::Path) -> String {
        path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    }

    /// The transient a write leaves on the status line (DESIGN.md §5.7):
    /// what changed, in which file, and that `z` undoes it. Every write
    /// used to end in silence.
    fn note_written(&mut self, path: &std::path::Path, what: &str) {
        self.transient = Some((view::written_note(&Self::file_word(path), what), std::time::Instant::now()));
    }

    /// A reload is one more cursor move (DESIGN.md §1): everything derived
    /// from the project is re-derived through the one home the open used,
    /// the cursor and the selection clamp, the block cursor clears, the
    /// bakes are discarded and rebuilt at the cursor, and the camera and the
    /// layout stay. A reload is not an open: the diagnose stance does not fire.
    fn adopt(&mut self, part: Part, check_result: CheckResult) {
        // A reload that turns the project red says so (DESIGN.md §5.7,
        // 2026-09-17): the diagnose stance never moves the view on a
        // reload, so the words are how the user hears it.
        if let Some(note) = view::turned_red_note(self.check_result.refusal.is_some(), check_result.refusal.is_some()) {
            self.transient = Some((note.to_string(), std::time::Instant::now()));
        }
        let d = derive(&part, check_result, &self.theme);
        let (k, block, sel) = view::after_reload(
            self.op_cursor,
            self.part.operations.len(),
            self.selection,
            part.operations.len(),
            d.edges.len(),
            d.rows.len(),
            self.review_rows.len(),
            &d.outline.tools.iter().map(|t| t.num).collect::<Vec<_>>(),
        );
        let k = self.cursor_after_write.take().map_or(k, |c| c.min(part.operations.len()));
        self.part = part;
        self.pattern_table = None; // the project changed: every row is stale
        self.pattern_rx = None; // and a table still being measured was measured on the old project
        self.walk = None;
        self.walk_rx = None;
        self.project_mtime = view::newest_mtime(&self.part.sources);
        self.project_note = None;
        // A write's refusal stands until the next write or reload (§5.7);
        // a half-typed field is the old program's.
        self.edit_note = None;
        self.field_edit.clear();
        self.outline = d.outline;
        self.paths = d.paths;
        self.edges = d.edges;
        self.fit_bb = d.fit_bb;
        self.rows = d.rows;
        self.annotations = d.annotations;
        self.gcode = d.gcode;
        self.clock = d.clock;
        self.emitted_moves = std::sync::Arc::new(d.emitted_moves);
        self.fixture = d.fixture;
        self.check_result = d.check_result_kept;
        self.op_cursor = k;
        self.block_cursor = block;
        self.selection = sel;
        self.scroll_program_to_block = false;
        self.removal = (k, view::removal_boundaries(&self.part, k));
        // The bakes in hand and in flight are the old program's: dropped,
        // receivers included — a worker still running sends into nothing.
        self.sim = None;
        self.sim_rx = None;
        self.sim_cursor = k;
        self.cells = None;
        self.cells_rx = None;
        self.immersion = None;
        self.immersion_blocks = None;
        // The rows read the program (in_project), so they follow it; the
        // suppressed lines read the files, so they follow those.
        self.refresh_review_rows();
        self.suppressed = scan_suppressed(&self.part);
        // A Create's own thing, selected now the edges and rows are the new
        // project's (ROADMAP item 3 slice 1); its card is editable at once.
        if let Some(made) = self.select_after_write.take() {
            let sel = match made {
                Made::Feature(name) => self.edges.iter().position(|e| e.name == name).map(view::Selection::Feature),
                Made::Operation(i) => (i < self.part.operations.len()).then_some(view::Selection::Operation(i)),
                Made::Tool(n) => Some(view::Selection::Tool(n)),
            };
            if let Some(sel) = sel {
                self.set_selection(sel);
            }
        }
    }

    /// The statement's form (DESIGN.md §5.4, Phase 21 slice 1): one row
    /// per key the kind takes — the file's value where the line carries
    /// it, a blank where it does not, optional keys marked as the
    /// creation form marks them. Enter commits through the sixth write:
    /// a value sets or appends, an emptied optional removes, an emptied
    /// required key is refused by name. A refusal by the form itself
    /// (a block statement, an undeclared tool) is one weak sentence.
    fn fields_form(&mut self, ui: &mut egui::Ui, target: FieldTarget, form: &Result<view::StatementForm, String>) {
        let form = match form {
            Ok(f) => f,
            Err(reason) => {
                ui.label(egui::RichText::new(reason).small().weak());
                return;
            }
        };
        let mut commit: Option<(String, String)> = None;
        let units_mm = self.part.units_mm;
        // Apply beside every draft (DESIGN.md §5.4, Phase 50 slice 5): a
        // mouse-first hand need not know Enter writes.
        let drafts_here = self.field_edit.iter().filter(|(t, _, _)| *t == target).count();
        let mut commit_all = false;
        // The keys the line carries come first; the rest of what the kind
        // takes is one fold away. An operation's card used to open on a
        // dozen empty fields, each tagged "optional", with the keys in
        // use scattered among them.
        let drafted = |key: &str| self.field_edit.iter().any(|(t, k, _)| *t == target && k == key);
        let (carried, spare): (Vec<usize>, Vec<usize>) = (0..form.fields.len()).partition(|&i| {
            let f = &form.fields[i];
            f.value.is_some() || f.required
        });
        // A key being typed stays where it stands, in the fold: moving it
        // up on the first character took the keyboard from it. The fold is
        // held open while one of its keys has a draft, so none is hidden.
        let spare_drafted = spare.iter().any(|&i| drafted(&form.fields[i].key));
        let mut row = |this: &mut Self, ui: &mut egui::Ui, i: usize| {
            let field = &form.fields[i];
            let key = &field.key;
            let value = field.value.clone().unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(view::field_label(key, units_mm)).small());
                // A reference field is a menu of what the project declares
                // (DESIGN.md §5.4): no free text, no filter by kind, and a
                // pick commits at once — a pick is a whole value.
                if let Some(options) = &field.options {
                    // An empty inventory is not a menu: it says so and
                    // names the door (the review's finding — a project with
                    // no tool could not schedule its first operation).
                    if options.is_empty() {
                        ui.label(egui::RichText::new(view::empty_inventory_note(key)).small());
                        return;
                    }
                    let mut picked = value.clone();
                    egui::ComboBox::from_id_salt(("reference", i, key.as_str()))
                        .selected_text(if picked.is_empty() { "pick…" } else { picked.as_str() })
                        .show_ui(ui, |ui| {
                            for o in options {
                                ui.selectable_value(&mut picked, o.clone(), o);
                            }
                        });
                    // No "optional" or "the only one declared" beside the
                    // menu: a key the line need not carry is under More
                    // keys, and the menu shows how many it offers.
                    if picked != value {
                        commit = Some((key.clone(), picked));
                    }
                    return;
                }
                let draft = this.field_edit.iter().find(|(t, k, _)| *t == target && k == key).map(|(_, _, b)| b.clone());
                let editing = draft.is_some();
                let mut buf = draft.unwrap_or_else(|| value.clone());
                // A draft is visibly a draft (DESIGN.md §5.4): its text in
                // the selection colour while it differs from the file's
                // value, so a click away cannot leave it looking written.
                let is_draft = editing && buf != value;
                let mut edit = egui::TextEdit::singleline(&mut buf).desired_width(140.0);
                if is_draft {
                    edit = edit.text_color(rgb(this.theme.select));
                }
                let r = ui.add(edit);
                if is_draft && ui.small_button("Apply").on_hover_text("write this value — what Enter does").clicked() {
                    commit = Some((key.clone(), buf.clone()));
                }
                if r.changed() {
                    // This field's draft, beside any other field's — a
                    // draft left in one field stands while another is typed.
                    this.field_edit.retain(|(t, k, _)| !(*t == target && k == key));
                    this.field_edit.push((target.clone(), key.clone(), buf.clone()));
                }
                let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
                if r.lost_focus() && enter && buf != value {
                    commit = Some((key.clone(), buf));
                } else if r.lost_focus() && esc && editing {
                    // Esc drops this field's draft; it shows the file's value again.
                    this.field_edit.retain(|(t, k, _)| !(*t == target && k == key));
                }
            });
        };
        for &i in &carried {
            row(self, ui, i);
        }
        if !spare.is_empty() {
            let title = format!("More keys ({})", spare.len());
            egui::CollapsingHeader::new(egui::RichText::new(title).small()).id_salt(("more keys", &form.head)).default_open(false).open(spare_drafted.then_some(true)).show(ui, |ui| {
                for &i in &spare {
                    row(self, ui, i);
                }
            });
        }
        if drafts_here >= 2 && ui.small_button("Apply all").on_hover_text("write every draft on this card, one after another").clicked() {
            commit_all = true;
        }
        if commit_all {
            let all: Vec<(String, String)> = self.field_edit.iter().filter(|(t, _, _)| *t == target).map(|(_, k, d)| (k.clone(), d.clone())).collect();
            for (key, draft) in all {
                match view::field_write(form, &key, &draft) {
                    Ok(value) => self.set_field(target.clone(), &key, value.as_deref()),
                    Err(e) => {
                        self.field_edit.retain(|(t, k, _)| !(*t == target && *k == key));
                        self.edit_note = Some(format!("Not written — {e}"));
                    }
                }
            }
        }
        if let Some((key, draft)) = commit {
            // The form's own refusal (a required key emptied) stands where
            // the parser's would (§5.7), and the draft is dropped with it.
            match view::field_write(form, &key, &draft) {
                Ok(value) => self.set_field(target, &key, value.as_deref()),
                Err(e) => {
                    self.field_edit.retain(|(t, k, _)| !(*t == target && *k == key));
                    self.edit_note = Some(format!("Not written — {e}"));
                }
            }
        }
    }

    /// The ninth write's door (DESIGN.md §5.4): the creation form with a
    /// run head on the selected feature — its `op_lines` are the menu.
    fn new_operation_on_selected(&mut self) {
        let view::Selection::Feature(i) = self.selection else {
            self.edit_note = Some("Not written — select a feature to create an operation on".to_string());
            return;
        };
        let Some(name) = self.edges.get(i).map(|e| e.name.clone()) else { return };
        match crate::template::op_lines(&self.part, &name) {
            Ok(skeletons) => {
                let mut form = NewForm::for_run(name, skeletons, 0);
                prefill_run(&self.part, &mut form);
                self.new_form = Some(form);
                // The door opens with the form (DESIGN.md §5.4).
                self.layout.apply(DockAction::OpenForm);
            }
            Err(e) => self.edit_note = Some(format!("Not written — {e}")),
        }
    }

    /// The seventh write's form (DESIGN.md §1, §5.4): a variant menu that
    /// mirrors the vocabulary (`template::FEATURE_SPECS` is the mirror),
    /// the variant's own keys as fields, a points field for a block
    /// variant. The form only gathers answers — the wizard's one home
    /// renders them and `create_feature` lands them.
    fn new_feature_form(&mut self, ui: &mut egui::Ui) {
        let mut create = false;
        let mut cancel = false;
        let lowest = crate::template::lowest_undeclared_tool(&self.part);
        // The creation form's own reference menus (DESIGN.md §5.4): a
        // blank whose key is a reference offers the declared inventory.
        let reference = |key: &str| view::reference_options(&self.part, key);
        let main_path = std::path::Path::new(&self.project_path).join("main.part");
        let units_mm = self.part.units_mm;
        let suggest = |variant: &str| crate::template::suggested_name(&self.part, variant);
        let menus: Vec<(String, Vec<String>)> = ["tool", "surface", "profile"]
            .iter()
            .filter_map(|k| reference(k).map(|o| (k.to_string(), o)))
            .collect();
        if let Some(form) = &mut self.new_form {
            // The head: a feature variant, a tool kind (the eighth write)
            // or a feature's skeleton (the ninth). Each menu mirrors its
            // own vocabulary; the fields are (key, required) rows.
            let (keys, points): (Vec<(String, bool)>, crate::template::Points) = match form.head {
                NewHead::Feature(v) => {
                    let specs = crate::template::FEATURE_SPECS;
                    ui.label(egui::RichText::new("New feature").strong());
                    let mut picked = v;
                    egui::ComboBox::from_id_salt("new-feature-variant")
                        .selected_text(specs[v].variant)
                        .show_ui(ui, |ui| {
                            // Grouped by what is being made (DESIGN.md §5.4,
                            // Phase 50 slice 5): `template::FEATURE_GROUPS`.
                            for (group, variants) in crate::template::FEATURE_GROUPS {
                                ui.label(egui::RichText::new(*group).small().weak());
                                for name in variants.iter() {
                                    if let Some(i) = specs.iter().position(|s| s.variant == *name) {
                                        ui.selectable_value(&mut picked, i, specs[i].variant);
                                    }
                                }
                            }
                        });
                    if picked != v {
                        *form = NewForm::for_variant(picked);
                        form.name = suggest(specs[picked].variant);
                    }
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("name =").small());
                        ui.add(egui::TextEdit::singleline(&mut form.name).desired_width(140.0));
                    });
                    let NewHead::Feature(v) = form.head else { unreachable!() };
                    (specs[v].keys.iter().map(|k| (k.name.to_string(), k.required)).collect(), specs[v].points)
                }
                NewHead::Run { ref feature, ref skeletons, pick } => {
                    ui.label(egui::RichText::new(format!("New operation on {feature}")).strong());
                    let mut picked = pick;
                    egui::ComboBox::from_id_salt("new-run-skeleton")
                        .selected_text(skeletons.get(pick).map(String::as_str).unwrap_or(""))
                        .width(320.0)
                        .show_ui(ui, |ui| {
                            for (i, s) in skeletons.iter().enumerate() {
                                ui.selectable_value(&mut picked, i, s);
                            }
                        });
                    if picked != pick {
                        *form = NewForm::for_run(feature.clone(), skeletons.clone(), picked);
                        prefill_run(&self.part, form);
                    }
                    let NewHead::Run { ref skeletons, pick, .. } = form.head else { unreachable!() };
                    // The blanks are the questions, every one required.
                    let blanks = skeletons.get(pick).map(|s| crate::template::skeleton_blanks(s)).unwrap_or_default();
                    (blanks.into_iter().map(|(k, _)| (k, true)).collect(), crate::template::Points::None)
                }
                NewHead::Project(k) => {
                    let kinds = crate::template::PROJECT_STOCK_KINDS;
                    ui.label(egui::RichText::new("New project").strong());
                    ui.label(egui::RichText::new(format!("writes {}", main_path.display())).small().weak());
                    let mut picked = k;
                    egui::ComboBox::from_id_salt("new-project-stock")
                        .selected_text(kinds[k])
                        .show_ui(ui, |ui| {
                            for (i, s) in kinds.iter().enumerate() {
                                ui.selectable_value(&mut picked, i, *s);
                            }
                        });
                    if picked != k {
                        *form = NewForm::for_project(picked);
                    }
                    let NewHead::Project(k) = form.head else { unreachable!() };
                    let keys = crate::template::project_keys(kinds[k]).unwrap_or_default();
                    (keys.into_iter().map(|(key, required, _)| (key.to_string(), required)).collect(), crate::template::Points::None)
                }
                NewHead::Tool(k) => {
                    let specs = crate::template::TOOL_SPECS;
                    ui.label(egui::RichText::new("New tool").strong());
                    let mut picked = k;
                    egui::ComboBox::from_id_salt("new-tool-kind")
                        .selected_text(specs[k].kind)
                        .show_ui(ui, |ui| {
                            for (i, s) in specs.iter().enumerate() {
                                ui.selectable_value(&mut picked, i, s.kind);
                            }
                        });
                    if picked != k {
                        *form = NewForm::for_tool(picked, lowest);
                    }
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("number =").small());
                        ui.add(egui::TextEdit::singleline(&mut form.name).desired_width(140.0));
                        ui.label(egui::RichText::new(format!("T{lowest} is the lowest undeclared")).small().weak());
                    });
                    let NewHead::Tool(k) = form.head else { unreachable!() };
                    (crate::template::tool_keys(&specs[k]).iter().map(|k| (k.name.to_string(), k.required)).collect(), crate::template::Points::None)
                }
            };
            let is_run = matches!(form.head, NewHead::Run { .. });
            for (i, ((key, required), buf)) in keys.iter().zip(form.values.iter_mut()).enumerate() {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(view::field_label(key, units_mm)).small());
                    // On the run head a reference blank is a menu; on the
                    // feature and tool heads no key is a reference.
                    if let Some((_, options)) = menus.iter().find(|(k, _)| is_run && k == key) {
                        if options.is_empty() {
                            ui.label(egui::RichText::new(view::empty_inventory_note(key)).small());
                            return;
                        }
                        egui::ComboBox::from_id_salt(("new-reference", i))
                            .selected_text(if buf.is_empty() { "pick…" } else { buf.as_str() })
                            .show_ui(ui, |ui| {
                                for o in options {
                                    ui.selectable_value(buf, o.clone(), o);
                                }
                            });
                        if options.len() == 1 {
                            ui.label(egui::RichText::new("the only one declared").small().weak());
                        }
                        return;
                    }
                    ui.add(egui::TextEdit::singleline(buf).desired_width(140.0));
                    if !required {
                        ui.label(egui::RichText::new("optional").small().weak());
                    }
                    if is_run && key == "stepover" && !buf.is_empty() {
                        ui.label(egui::RichText::new("suggested — half the tool's diameter; Create commits it").small().weak());
                    }
                });
            }
            let spec_points = points;
            if spec_points != crate::template::Points::None {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("points =").small());
                    ui.add(egui::TextEdit::singleline(&mut form.points).desired_width(200.0));
                });
                ui.label(
                    egui::RichText::new(match spec_points {
                        crate::template::Points::Wire => "x,y,z per point, ';' between points",
                        _ => "x,y per point, ';' between points",
                    })
                    .small()
                    .weak(),
                );
                // The quick-start (DESIGN.md §5.4, Phase 50 slice 6): a
                // rectangle or a circle as numbers, expanded to the points
                // text and wound the way this kind wants.
                if spec_points == crate::template::Points::Planar {
                    let variant = match form.head {
                        NewHead::Feature(v) => crate::template::FEATURE_SPECS[v].variant,
                        _ => "",
                    };
                    let ccw = crate::template::winding_ccw(variant).unwrap_or(true);
                    ui.label(egui::RichText::new("or from numbers — wound the way this kind wants:").small().weak());
                    ui.horizontal(|ui| {
                        for (i, label) in ["w", "l", "dia", "x", "y"].iter().enumerate() {
                            ui.label(egui::RichText::new(*label).small());
                            ui.add(egui::TextEdit::singleline(&mut self.quick_shape[i]).desired_width(44.0));
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.small_button("rectangle → points").on_hover_text("w by l with its corner at x, y (the origin when blank)").clicked() {
                            match crate::template::rectangle_points(&self.quick_shape[0], &self.quick_shape[1], &self.quick_shape[3], &self.quick_shape[4], ccw) {
                                Ok(text) => form.points = text,
                                Err(e) => self.edit_note = Some(format!("Not made — {e}")),
                            }
                        }
                        if ui.small_button("circle → points").on_hover_text("dia centred at x, y, thirty-six chords").clicked() {
                            match crate::template::circle_points(&self.quick_shape[2], &self.quick_shape[3], &self.quick_shape[4], 36, ccw) {
                                Ok(text) => form.points = text,
                                Err(e) => self.edit_note = Some(format!("Not made — {e}")),
                            }
                        }
                    });
                }
            }
            let word = match form.head {
                NewHead::Feature(_) => "Create feature",
                NewHead::Tool(_) => "Create tool",
                NewHead::Run { .. } => "Create operation",
                NewHead::Project(_) => "Create project",
            };
            let hover = if matches!(form.head, NewHead::Project(_)) {
                "Write main.part here — the header and the stock, checked through the one parser first; the window then opens on it. Not a project edit: z does not undo it"
            } else {
                "Write this statement to main.part. z undoes."
            };
            ui.horizontal(|ui| {
                create = ui
                    .button(word)
                    .on_hover_text(hover)
                    .clicked();
                cancel = ui.button("Cancel").clicked();
            });
        }
        if create {
            self.create_feature();
        } else if cancel {
            self.new_form = None;
        }
    }

    /// The seventh write (DESIGN.md §1): the form's answers through the
    /// wizard's one home, the whole project re-parsed in memory with the
    /// statement appended, then the accept path — `append_statement`, the
    /// atomic write, the undo stack, the reload. Two refusals are the
    /// window's own because the parser cannot make them loudly: a name
    /// the project already declares (the name-keyed map would silently
    /// shadow one of the two), and a project whose `geometry.part`
    /// carries the feature declarations (the destination is ambiguous,
    /// and the window does not pick files).
    fn create_feature(&mut self) {
        let Some(form) = &self.new_form else { return };
        if let NewHead::Project(k) = form.head {
            self.create_project(k);
            return;
        }
        let name = form.name.trim().to_string();
        // A composed operation lands AFTER THE CURSOR (Phase 61 slice 3), not
        // at the end of the file; every other statement appends as ever.
        let insert_at = matches!(form.head, NewHead::Run { .. }).then(|| self.op_cursor.min(self.part.operations.len()));
        let ops_before = self.part.operations.len();
        let what = match &form.head {
            NewHead::Feature(_) => format!("feature {name} created"),
            NewHead::Tool(_) => format!("tool T{name} created"),
            NewHead::Run { feature, .. } => format!("operation on {feature} created"),
            NewHead::Project(_) => unreachable!("handled above"),
        };
        // What lands is what is selected next (ROADMAP item 3 slice 1): the
        // form closes and the new thing's card takes its place. Until
        // 2026-11-07 a feature with another skeleton reopened the form on
        // the next one, and only Esc closed it.
        let made = match &form.head {
            NewHead::Feature(_) => Some(Made::Feature(name.clone())),
            NewHead::Tool(_) => crate::template::parse_tool_number(&name).ok().map(Made::Tool),
            NewHead::Run { .. } => Some(Made::Operation(insert_at.unwrap_or(ops_before))),
            NewHead::Project(_) => unreachable!("handled above"),
        };
        let keys: Vec<String> = match &form.head {
            NewHead::Feature(v) => crate::template::FEATURE_SPECS[*v].keys.iter().map(|k| k.name.to_string()).collect(),
            NewHead::Tool(k) => crate::template::tool_keys(&crate::template::TOOL_SPECS[*k]).iter().map(|k| k.name.to_string()).collect(),
            NewHead::Run { skeletons, pick, .. } => {
                skeletons.get(*pick).map(|s| crate::template::skeleton_blanks(s).into_iter().map(|(k, _)| k).collect()).unwrap_or_default()
            }
            NewHead::Project(_) => unreachable!("handled above"),
        };
        let mut answers: Vec<(String, String)> = keys
            .iter()
            .zip(form.values.iter())
            .filter(|(_, v)| !v.trim().is_empty())
            .map(|(k, v)| (k.clone(), v.trim().to_string()))
            .collect();
        let rendered = match form.head.clone() {
            NewHead::Feature(v) => {
                let spec = &crate::template::FEATURE_SPECS[v];
                if spec.points != crate::template::Points::None && !form.points.trim().is_empty() {
                    answers.push(("points".to_string(), form.points.trim().to_string()));
                }
                if self.part.features.contains_key(&name) {
                    self.edit_note = Some(format!(
                        "Not written — a feature named '{name}' is already declared; the parser would silently shadow one of the two"
                    ));
                    return;
                }
                if view::second_file_declares_features(&self.part) {
                    self.edit_note = Some(
                        "Not written — geometry.part carries the feature declarations, so the destination is ambiguous; declare it there by hand".to_string(),
                    );
                    return;
                }
                crate::template::feature_statement(spec.variant, &name, &answers)
            }
            // The eighth write: a duplicate number is the parser's own loud
            // refusal (made by the re-parse below), and since fork 7 a tool
            // line cannot live in geometry.part, so nothing is ambiguous.
            NewHead::Tool(k) => crate::template::parse_tool_number(&name)
                .and_then(|n| crate::template::tool_statement(crate::template::TOOL_SPECS[k].kind, n, &answers)),
            // The ninth write: the skeleton with its blanks filled; the
            // re-parse below judges the tool and everything else.
            NewHead::Project(_) => unreachable!("handled above"),
            NewHead::Run { skeletons, pick, .. } => skeletons
                .get(pick)
                .ok_or_else(|| "no skeleton picked".to_string())
                .and_then(|s| crate::template::fill_skeleton(s, &answers)),
        };
        let Some(path) = self.part.sources.first().cloned() else {
            self.edit_note = Some("Not written — this project has no file on disk to append to".to_string());
            return;
        };
        let land = |statement: &str| -> Result<(String, String), String> {
            let before = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let after = match insert_at {
                Some(at) => view::insert_run_line_at(&before, &self.part.op_lines, at, statement),
                None => view::append_statement(&before, statement),
            };
            // The one parser, in memory, on the whole project as it
            // would be — geometry.part by NAME, never whatever file
            // happens to sit second in the source list.
            let geom = self
                .part
                .sources
                .get(1)
                .filter(|g| g.file_name().is_some_and(|n| n == "geometry.part"))
                .and_then(|g| std::fs::read_to_string(g).ok());
            let base = std::path::Path::new(&self.project_path);
            let base = if base.is_dir() { Some(base) } else { base.parent() };
            crate::parse::parse_project_with_base(&after, geom.as_deref(), base)?;
            crate::theme::write_atomically(&path, &after)?;
            Ok((before, after))
        };
        let mut result = rendered.and_then(|s| land(&s));
        // A loop the wrong way round (DESIGN.md §5.4, Phase 50 slice 6): the
        // form reverses the points IT authored and lands them again, and
        // says so. The parser's law stands — a file's traversal is the
        // author's and the cut's — this is the form's act on its own answer.
        let mut reversed: Option<&str> = None;
        if let (Err(e), NewHead::Feature(v)) = (&result, &form.head) {
            let spec = &crate::template::FEATURE_SPECS[*v];
            if spec.points == crate::template::Points::Planar && e.contains("winds") {
                let mut again = answers.clone();
                if let Some(pts) = again.iter_mut().find(|(k, _)| k == "points") {
                    pts.1 = view::reverse_points(&pts.1);
                }
                if let Ok(s) = crate::template::feature_statement(spec.variant, &name, &again) {
                    result = land(&s);
                    if result.is_ok() {
                        reversed = Some(if spec.variant == "ProfileBoss" { "CW, a boss is cut from outside" } else { "CCW, a pocket is cut from inside" });
                    }
                }
            }
        }
        let what = match reversed {
            Some(way) => format!("{what} — points reversed to wind {way}"),
            None => what,
        };
        match result {
            Ok((before, after)) => {
                let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                self.note_written(&path, &what);
                self.edits.record(view::Edit { path, before, after, stamp });
                self.edit_note = None;
                // Landed mid-program: the cursor follows the new line.
                if let Some(at) = insert_at.filter(|at| *at < ops_before) {
                    self.cursor_after_write = Some(at + 1);
                }
                self.new_form = None;
                self.select_after_write = made;
            }
            Err(e) => self.edit_note = Some(format!("Not written — {e}")),
        }
    }

    /// The project's first file (DESIGN.md §1, Phase 50 slice 3): the
    /// form's answers through `template::project_text`, the one parser on
    /// the text, the directory made, `main.part` written through the
    /// atomic rename, and the next poll opening on it. Not a write under
    /// the one law — it makes the file the law is about — so not on the
    /// undo stack. Refused by name over a `main.part` that stands.
    fn create_project(&mut self, kind: usize) {
        let Some(form) = &self.new_form else { return };
        let kind_word = crate::template::PROJECT_STOCK_KINDS[kind];
        let keys = crate::template::project_keys(kind_word).unwrap_or_default();
        let answers: Vec<(String, String)> = keys
            .iter()
            .zip(form.values.iter())
            .filter(|(_, v)| !v.trim().is_empty())
            .map(|((k, _, _), v)| (k.to_string(), v.trim().to_string()))
            .collect();
        let given = std::path::Path::new(&self.project_path);
        let dir = if given.extension().is_some_and(|e| e == "part") { given.parent().map(|p| p.to_path_buf()).unwrap_or_default() } else { given.to_path_buf() };
        let main = dir.join("main.part");
        if main.exists() {
            self.edit_note = Some(format!("Not written — {} exists; a project is edited, not written over", main.display()));
            return;
        }
        let result = crate::template::project_text(kind_word, &answers)
            .and_then(|text| crate::parse::parse(&text).map(|_| text))
            .and_then(|text| {
                std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
                crate::theme::write_atomically(&main, &text)
            });
        match result {
            Ok(()) => {
                self.transient = Some((format!("Written — main.part: a {kind_word} stock and the header"), std::time::Instant::now()));
                self.edit_note = None;
                self.new_form = None;
                self.project_reload = true;
            }
            Err(e) => self.edit_note = Some(format!("Not written — {e}")),
        }
    }

    /// The header's write (DESIGN.md §5.4, Phase 50 slice 3): one `key
    /// value` line rewritten where it stands, or inserted before the stock
    /// line when the file lacks it — the same in-memory re-parse, atomic
    /// write, undo stack and reload as the sixth write. A header line is
    /// never removed from the form: the header law needs every one.
    fn set_header(&mut self, key: &str, value: Option<&str>) {
        self.field_edit.retain(|(t, k, _)| !(*t == FieldTarget::Header && k == key));
        let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
            self.edit_note = Some(format!("Not written — the header's `{key}` is not removed; every program needs it"));
            return;
        };
        let what = format!("{key} {value}");
        let Some(path) = self.part.sources.first().cloned() else {
            self.edit_note = Some("Not written — this project has no file on disk".to_string());
            return;
        };
        let plan = view::header_write_plan(&self.part, key);
        let at = view::header_insert_at(&self.part);
        let new_line = view::header_line(key, value);
        let result = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))
            .and_then(|before| {
                let after = match plan {
                    Some(line) => view::replace_line(&before, line, &new_line)?,
                    None => view::insert_line(&before, at, &new_line),
                };
                self.reparse_with(0, &after)?;
                crate::theme::write_atomically(&path, &after)?;
                Ok((before, after))
            });
        match result {
            Ok((before, after)) => {
                let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                self.note_written(&path, &what);
                self.edits.record(view::Edit { path, before, after, stamp });
                self.edit_note = None;
            }
            Err(e) => self.edit_note = Some(format!("Not written — {e}")),
        }
    }

    /// The sixth write (DESIGN.md §1): one `key=value` on the target's own
    /// line — set, appended, or (`None`) removed — the whole project
    /// re-parsed in memory with the new line in place before anything is
    /// written: a value the parser refuses is refused here with the
    /// parser's sentence and the file untouched.
    fn set_field(&mut self, target: FieldTarget, key: &str, value: Option<&str>) {
        if target == FieldTarget::Header {
            self.set_header(key, value);
            return;
        }
        self.set_fields(target, &[(key, value)]);
    }

    /// Several keys of ONE statement written as one edit (Phase 62 slice 3):
    /// the line takes every write before the one reparse, so a pair that
    /// validates only together — `pattern=held` with its stepover dropped —
    /// lands, where key by key the first would be refused ("two governors")
    /// and nothing written. One file write, one undo step, one note.
    fn set_fields(&mut self, target: FieldTarget, writes: &[(&str, Option<&str>)]) {
        for (key, _) in writes {
            self.field_edit.retain(|(t, k, _)| !(*t == target && k == key));
        }
        let what = writes
            .iter()
            .map(|(key, value)| match value {
                Some(v) => format!("{key} = {v}"),
                None => format!("{key} removed"),
            })
            .collect::<Vec<_>>()
            .join(", ");
        let found = match &target {
            FieldTarget::Op(i) => self.part.op_lines.get(*i).copied(),
            FieldTarget::Feature(name) => self.part.feature_lines.iter().find(|(n, ..)| n == name).map(|(_, f, l, _)| (*f, *l)),
            FieldTarget::Tool(t) => self.part.tool_lines.iter().find(|(n, ..)| n == t).map(|(_, f, l, _)| (*f, *l)),
            FieldTarget::Stock => self.part.stock_line.as_ref().map(|(f, l, _)| (*f, *l)),
            FieldTarget::Header => None,
        };
        let Some((file, line)) = found else {
            self.edit_note = Some("Not written — that statement has no line on disk".to_string());
            return;
        };
        let Some(path) = self.part.sources.get(file).cloned() else {
            self.edit_note = Some("Not written — that file is not on disk".to_string());
            return;
        };
        let result = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))
            .and_then(|before| {
                let old_line = before.lines().nth(line).ok_or_else(|| format!("line {} is past the end of the file", line + 1))?;
                let mut new_line = old_line.to_string();
                for (key, value) in writes {
                    new_line = view::set_key(&new_line, key, value.map(str::trim))?;
                }
                let after = view::replace_line(&before, line, &new_line)?;
                // The one parser, in memory, on the whole project as it
                // would be (`reparse_with`).
                self.reparse_with(file, &after)?;
                crate::theme::write_atomically(&path, &after)?;
                Ok((before, after))
            });
        match result {
            Ok((before, after)) => {
                let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                self.note_written(&path, &what);
                self.edits.record(view::Edit { path, before, after, stamp });
                self.edit_note = None;
            }
            Err(e) => self.edit_note = Some(format!("Not written — {e}")),
        }
    }

    /// The patterns an op could be walked in, as `nacs --patterns` prints them,
    /// with a pin beside each (Phase 28 slice 5). Thin: the rows are the view
    /// model's, the pin is `set_field` — one word written to the run line.
    /// The walk as cut, on the card (ROADMAP item 3 slice 4): measured on a
    /// worker like the table, keyed by (op, line); the words are
    /// `compare::walk_lines`' first line, the picture the viewport's.
    fn walk_card(&mut self, ui: &mut egui::Ui, i: usize) {
        let line = self.part.op_texts.get(i).cloned().unwrap_or_default();
        let fresh = |entry: &(usize, String, Option<crate::compare::Walked>)| entry.0 == i && entry.1 == line;
        if !self.walk.as_ref().is_some_and(fresh) {
            if self.walk_rx.is_none() {
                let (tx, rx) = std::sync::mpsc::channel();
                let part = self.part.clone();
                let emitted = self.emitted_moves.clone();
                let key = line.clone();
                std::thread::spawn(move || {
                    let _ = tx.send((i, key, crate::compare::walk_as_emitted(&part, &emitted, i)));
                });
                self.walk_rx = Some(rx);
            }
            if let Some(rx) = &self.walk_rx {
                match rx.try_recv() {
                    Ok(result) => {
                        self.walk = Some(result);
                        self.walk_rx = None;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.walk_rx = None;
                    }
                }
            }
            if !self.walk.as_ref().is_some_and(fresh) {
                ui.label(egui::RichText::new("as cut: measuring…").small().weak());
                return;
            }
        }
        match self.walk.as_ref().and_then(|(_, _, w)| w.as_ref()) {
            Some(w) => {
                let lines = crate::compare::walk_lines(w);
                ui.add(egui::Label::new(format!("{} — drawn on the path", lines[0])).wrap()).on_hover_text(lines[1..].join("\n"));
            }
            None => {
                ui.label(egui::RichText::new("as cut: the walk cannot read this op — it works no floor region, or the stock before it is not modelled in closed form").small().weak());
            }
        }
    }

    fn pattern_card(&mut self, ui: &mut egui::Ui, i: usize) {
        let line = self.part.op_texts.get(i).cloned().unwrap_or_default();
        let fresh = |entry: &(usize, String, Option<view::PatternTable>)| entry.0 == i && entry.1 == line;
        if !self.pattern_table.as_ref().is_some_and(fresh) {
            // Stale: measure on a worker. A worker already in flight for
            // another op is left to finish and its result kept — it is keyed
            // by (op, line), so a later paint of that op finds it fresh.
            if self.pattern_rx.is_none() {
                let (tx, rx) = std::sync::mpsc::channel();
                let part = self.part.clone();
                let key = line.clone();
                std::thread::spawn(move || {
                    let _ = tx.send((i, key, view::pattern_table(&part, i)));
                });
                self.pattern_rx = Some(rx);
            }
            if let Some(rx) = &self.pattern_rx {
                match rx.try_recv() {
                    Ok(result) => {
                        self.pattern_table = Some(result);
                        self.pattern_rx = None;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.pattern_rx = None;
                    }
                }
            }
            if !self.pattern_table.as_ref().is_some_and(fresh) {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(12.0));
                    ui.label(egui::RichText::new("Patterns — measuring… (each pattern runs through the emitter)").small().weak());
                });
                return;
            }
        }
        let Some((_, _, Some(table))) = self.pattern_table.clone() else { return };
        ui.separator();
        ui.label(egui::RichText::new("Patterns — measured, never scored").strong());
        for land in &table.lands {
            ui.label(egui::RichText::new(land).small().weak());
        }
        ui.label(egui::RichText::new(table.note).small().weak());
        // The table as a grid of the terminal's own cells (Phase 39 slice 2):
        // the columns are `compare::COLUMNS`, the cells `compare::cells`, and
        // nothing here decides a word — only where it sits. A cell never
        // wraps; a card narrower than the table scrolls sideways.
        let mut pin_clicked: Option<&'static str> = None;
        // The table's bar stays with the table (Phase 59 slice 1, fork
        // 0A). egui moves a nested area's bar up to the outer clip's edge
        // whenever the area's own edge is below it ("Move the scrollbar so
        // it is visible", `scroll_area.rs`), so with the card at its top
        // the table's bar stood at the dock's bottom with the table five
        // hundred pixels under it — a bar for nothing in view (the Phase
        // 58 walk). A table that straddles the dock's bottom keeps the
        // relocated bar: it lies over the table's own visible rows, where
        // it belongs. A table whose top is at or past the clip's bottom
        // gets no bar — the content is laid out as ever, so the card's
        // height and scroll range do not move. One comparison: the DRAFT
        // reached for a child ui and a remembered height, and neither is
        // needed, because the relocation already does the straddling case.
        let table_below = ui.cursor().top() >= ui.clip_rect().bottom();
        let bar = if table_below { egui::scroll_area::ScrollBarVisibility::AlwaysHidden } else { egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded };
        egui::ScrollArea::horizontal().id_salt(("patterns", i)).scroll_bar_visibility(bar).show(ui, |ui| {
            let mono = |s: &str| egui::RichText::new(s).monospace().small();
            let cell = |ui: &mut egui::Ui, left: bool, label: egui::Label| {
                if left {
                    ui.add(label);
                } else {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add(label);
                    });
                }
            };
            egui::Grid::new(("patterns", i)).num_columns(table.columns.len() + 1).spacing([10.0, 2.0]).show(ui, |ui| {
                // The group line: a group's name over its first column.
                ui.label("");
                let mut prev = "";
                for c in table.columns {
                    ui.add(egui::Label::new(mono(if c.group != prev { c.group } else { "" }).weak()).extend());
                    prev = c.group;
                }
                ui.end_row();
                ui.label("");
                for c in table.columns {
                    cell(ui, c.left, egui::Label::new(mono(c.title).weak()).extend());
                }
                ui.end_row();
                for row in &table.rows {
                    let pin = ui.add_enabled(row.pinnable && !row.current, egui::Button::new("pin").small());
                    if pin.on_hover_text("Use this pattern for the operation. z undoes.").clicked() {
                        pin_clicked = Some(row.word);
                    }
                    let mut k = 0;
                    for c in &row.cells {
                        // The current row carries the terminal's mark and reads strong.
                        let text = if k == 0 && row.current { format!("* {}", c.text) } else { c.text.clone() };
                        let rich = if row.current { mono(&text).strong() } else { mono(&text) };
                        cell(ui, table.columns[k].left, egui::Label::new(rich).extend());
                        for _ in 1..c.span {
                            ui.label("");
                        }
                        k += c.span;
                    }
                    ui.end_row();
                }
            });
        });
        if let Some(word) = pin_clicked {
            // The pin's writes (Phase 62 slices 1 and 3): the word, and the
            // stepover `held` refuses or the others need — as ONE edit, since
            // the pair validates only together.
            let writes = view::pin_writes(&self.part, i, word);
            let writes: Vec<(&str, Option<&str>)> = writes.iter().map(|(k, v)| (*k, v.as_deref())).collect();
            self.set_fields(FieldTarget::Op(i), &writes);
        }
    }

    /// The fifth write (DESIGN.md §1): a whole-program transform of
    /// `main.part`, through the same atomic write, undo stack and reload.
    fn transform_main(&mut self, what: &str, f: impl Fn(&str) -> Result<String, String>) {
        let Some(path) = self.part.sources.first().cloned() else {
            self.edit_note = Some(format!("Not written — {what}: this project has no file on disk"));
            return;
        };
        // No "which file" refusal any more (Phase 20, fork 7): the parser
        // refuses a run or tool line in geometry.part, so a loaded project
        // has exactly one file those lines live in.
        let result = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))
            .and_then(|before| f(&before).map(|after| (before, after)))
            .and_then(|(before, after)| {
                if after == before {
                    return Err(format!("{what}: nothing to change"));
                }
                crate::theme::write_atomically(&path, &after)?;
                Ok((before, after))
            });
        match result {
            Ok((before, after)) => {
                let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                self.note_written(&path, what);
                self.edits.record(view::Edit { path, before, after, stamp });
                self.edit_note = None;
            }
            Err(e) => self.edit_note = Some(format!("Not written — {e}")),
        }
    }

    /// The fourth write (DESIGN.md §1, §5.2): the operation's `run` line
    /// and its neighbour's exchange places — `view::swap_run_lines` on the
    /// file both live in — through the same atomic write, undo stack and
    /// reload. The selection is set to the neighbour's index first, so the
    /// reload's index-keeping clamp lands on the moved operation.
    fn reorder(&mut self, op: usize, down: bool) {
        let n = self.part.operations.len();
        let Some(neighbour) = (if down { (op + 1 < n).then_some(op + 1) } else { op.checked_sub(1) }) else {
            self.edit_note = Some(format!("Not written — operation [{}] is already at the {}", op, if down { "bottom" } else { "top" }));
            return;
        };
        let (Some(&(f_a, l_a)), Some(&(f_b, l_b))) = (self.part.op_lines.get(op), self.part.op_lines.get(neighbour)) else {
            self.edit_note = Some("Not written — this operation has no run line on disk".to_string());
            return;
        };
        if f_a != f_b {
            self.edit_note = Some("Not written — the two operations' run lines live in different part files".to_string());
            return;
        }
        let Some(path) = self.part.sources.get(f_a).cloned() else {
            self.edit_note = Some("Not written — that file is not on disk".to_string());
            return;
        };
        let result = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))
            .and_then(|before| view::swap_run_lines(&before, l_a, l_b).map(|after| (before, after)))
            .and_then(|(before, after)| {
                crate::theme::write_atomically(&path, &after)?;
                Ok((before, after))
            });
        match result {
            Ok((before, after)) => {
                let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                self.note_written(&path, &format!("operation [{}] moved {}", op, if down { "down" } else { "up" }));
                self.edits.record(view::Edit { path, before, after, stamp });
                self.edit_note = None;
                self.set_selection(view::Selection::Operation(neighbour));
                self.proposal_edges = None;
            }
            Err(e) => self.edit_note = Some(format!("Not written — {e}")),
        }
    }

    /// An operation to the top or the bottom (DESIGN.md §5.2, Phase 50
    /// slice 9): the fourth write's swap composed in memory
    /// (`view::move_run_line`), written once through the one atomic write,
    /// undo stack and reload.
    fn move_selected_op(&mut self, to_top: bool) {
        let view::Selection::Operation(op) = self.selection else {
            self.edit_note = Some("Not written — select an operation to move".to_string());
            return;
        };
        let Some(path) = self.part.op_lines.get(op).and_then(|(f, _)| self.part.sources.get(*f)).cloned() else {
            self.edit_note = Some("Not written — this operation has no run line on disk".to_string());
            return;
        };
        let result = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))
            .and_then(|before| view::move_run_line(&before, &self.part.op_lines, op, to_top).map(|after| (before, after)))
            .and_then(|(before, after)| {
                crate::theme::write_atomically(&path, &after)?;
                Ok((before, after))
            });
        match result {
            Ok((before, after)) => {
                let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                self.note_written(&path, &format!("operation [{}] moved to the {}", op, if to_top { "top" } else { "bottom" }));
                self.edits.record(view::Edit { path, before, after, stamp });
                self.edit_note = None;
                self.set_selection(view::Selection::Operation(if to_top { 0 } else { self.part.operations.len() - 1 }));
                self.proposal_edges = None;
            }
            Err(e) => self.edit_note = Some(format!("Not written — {e}")),
        }
    }

    /// The third write (DESIGN.md §1, §5.2): the `#` leaves the `run` line,
    /// through the same atomic write, undo stack and reload as suppress.
    fn restore(&mut self, file: usize, line: usize) {
        let Some(path) = self.part.sources.get(file).cloned() else {
            self.edit_note = Some("Not written — that file is not on disk".to_string());
            return;
        };
        let result = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))
            .and_then(|before| view::restore_line(&before, line).map(|after| (before, after)))
            .and_then(|(before, after)| {
                crate::theme::write_atomically(&path, &after)?;
                Ok((before, after))
            });
        match result {
            Ok((before, after)) => {
                let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                self.note_written(&path, "a suppressed run line restored");
                self.edits.record(view::Edit { path, before, after, stamp });
                self.edit_note = None;
            }
            Err(e) => self.edit_note = Some(format!("Not written — {e}")),
        }
    }

    /// Move the cursor. The declared stock is cheap and rebuilt at once; the
    /// swept stock and the cells are worker builds and rebuild lazily, when
    /// their switches are on and their `k` is stale.
    fn set_op_cursor(&mut self, k: usize) {
        let k = k.min(self.part.operations.len());
        if k == self.op_cursor {
            return;
        }
        self.op_cursor = k;
        // The nesting invariant (DESIGN.md §5.6): any change of the op
        // cursor, by any door, clears the block cursor.
        self.block_cursor = None;
        self.removal = (k, view::removal_boundaries(&self.part, k));
    }

    /// The rest scan at the cursor, built on a worker thread when its layer
    /// is on and the scan in hand is stale. Progress and failure report to
    /// the status line (DESIGN.md I10), never to a pane.
    fn poll_rest_material(&mut self, ctx: &egui::Context) {
        let stale = self.cells.as_ref().map_or(true, |(k, _)| *k != self.op_cursor);
        if stale && self.cells_rx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            let part = self.part.clone();
            let emitted_moves = self.emitted_moves.clone();
            let k = self.op_cursor;
            std::thread::spawn(move || {
                let _ = tx.send((k, view::rest_cells(&part, &emitted_moves, k).map(|c| std::sync::Arc::new(view::RestCellsView::of(c)))));
            });
            self.cells_rx = Some(rx);
        }
        if let Some(rx) = &self.cells_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.cells = Some(result);
                    self.cells_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.cells = Some((self.op_cursor, Err(String::from("Scan failed. See console for details."))));
                    self.cells_rx = None;
                }
            }
        }
    }

    /// The findings pane: the check's check result as rows —
    /// the error first, then every finding grouped by family in gate
    /// order, each in its family's colour with the sentence the terminal
    /// prints. Clicking a row goes to its annotation (the camera centres there),
    /// selects its op's toolpath and, for a finding about a feature, picks
    /// that feature's edge — the same selection and pick the outline and
    /// the viewport make. Nothing here is computed: `view::finding_rows`
    /// owns every word and colour.
    /// The account under the problems (ROADMAP item 2, slice 7): the lines
    /// `--check` prints, the summary first — what was checked, not only what
    /// was found.
    /// What was checked, folded: the band's line already says how many
    /// checks ran, so the family-by-family account is one click away and
    /// not eleven small lines under every list.
    fn account_lines(&self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        let (summary, lines) = view::account_lines(&self.account);
        egui::CollapsingHeader::new(egui::RichText::new(format!("What was checked — {summary}")).small()).id_salt("problems.account").default_open(false).show(ui, |ui| {
            for line in lines {
                ui.add(egui::Label::new(egui::RichText::new(line).small()).wrap());
            }
        });
    }

    fn problems_pane(&mut self, ui: &mut egui::Ui) {
        // A reload that failed is the first row (DESIGN.md §5.2, Phase 50
        // slice 7): the whole sentence, and the file for the desktop's
        // handler when the parser named a line. The last good program
        // stays up beneath it (§1).
        if let Some(note) = self.project_note.clone().filter(|n| n.starts_with("Not reloaded")) {
            ui.label(egui::RichText::new("NOT RELOADED").color(rgb(self.theme.refusal)).strong());
            ui.add(egui::Label::new(egui::RichText::new(&note).small()).wrap());
            if let Some((file, line)) = view::parse_error_place(&note) {
                let path = self.part.sources.iter().find(|s| s.file_name().is_some_and(|n| n == file)).cloned();
                if let Some(path) = path {
                    if ui.small_button(format!("Open {file}")).on_hover_text(format!("hand {} to the desktop's own handler — line {line} is the one; the window is not an editor", path.display())).clicked() {
                        if let Err(e) = view::open_in_editor(&path) {
                            self.edit_note = Some(e);
                        }
                    }
                }
            }
            ui.add_space(4.0);
        }
        if self.rows.is_empty() {
            // Nothing found is said once, on the band's line above.
            self.account_lines(ui);
            return;
        }
        let mut go: Option<usize> = None;
        let (mut pick, mut menu) = (None, None);
        let refusal = rgb(self.theme.refusal);
        for (i, row) in self.rows.iter().enumerate() {
            // How bad, by the head: a rejection in the refusal colour, a
            // finding plain and strong, a check that could not run weak.
            // The family's colour is a swatch, the same colour as its
            // mark in the viewport, so the two are matched by eye.
            let rejected = row.head == "REJECTED";
            let head = egui::RichText::new(&row.head).strong();
            let head = if rejected { head.color(refusal) } else if row.unchecked { head.weak() } else { head };
            let checked = self.selection == view::Selection::Finding(i);
            let resp = ui
                .horizontal(|ui| {
                    if !rejected {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, 2.0, rgb(row.color));
                    }
                    ui.selectable_label(checked, head)
                })
                .inner;
            if resp.clicked() {
                go = Some(i);
            }
            row_menu(&resp, view::Selection::Finding(i), &mut pick, &mut menu);
            // The gist here; the whole sentence is on the card.
            ui.add(egui::Label::new(egui::RichText::new(view::finding_gist(&row.text)).small()).wrap()).on_hover_text(&row.text);
            ui.add_space(4.0);
        }
        self.account_lines(ui);
        if let Some(i) = go {
            self.set_selection(view::Selection::Finding(i));
        }
        if let Some(sel) = pick {
            self.set_selection(sel);
        }
        if let Some(id) = menu {
            self.act(id);
        }
    }

    /// The job's steps as the window knows them: `view::job_steps` on the
    /// loaded project, report and proposals.
    fn job_steps(&self) -> Vec<view::JobStep> {
        // One stat a frame; the window repaints on input and twice a second.
        let has_file = self.part.sources.first().is_some_and(|p| p.is_file());
        let model = match &self.report {
            Some(Ok(r)) => Some(r.source.as_str()),
            _ => None,
        };
        let stock_waiting = matches!(&self.stock_proposal, Some(Ok(s)) if !s.in_project);
        let waiting = view::proposals_waiting(&self.review_rows, &self.review_dups);
        view::job_steps(&self.part, has_file, model, stock_waiting, waiting, self.check_result.refusal.as_deref())
    }

    /// The job guide at the top of the sidebar, over every tab, until the
    /// operations are scheduled: the six steps in order, the next one with
    /// its sentence and its button.
    fn job_guide(&mut self, ui: &mut egui::Ui) {
        let steps = self.job_steps();
        let next = steps.iter().position(|s| s.state == view::StepState::Next);
        // A project with its operations scheduled has no use for a guide:
        // the sidebar is the Outline's, and Write program is on the band.
        if next.is_none_or(|i| i + 1 == steps.len()) {
            return;
        }
        let title = match next {
            Some(i) => format!("Job: step {} of {}, {}", i + 1, steps.len(), steps[i].name),
            None => "Job".to_string(),
        };
        let mut act: Option<&'static str> = None;
        egui::CollapsingHeader::new(egui::RichText::new(title).strong()).id_salt("job.guide").default_open(true).show(ui, |ui| {
            for (i, s) in steps.iter().enumerate() {
                let head = format!("{}  {}", i + 1, s.name);
                match s.state {
                    view::StepState::Done => {
                        ui.add(egui::Label::new(egui::RichText::new(format!("{head}  · {}", s.says)).weak()).truncate());
                    }
                    view::StepState::Later => {
                        ui.label(egui::RichText::new(head).weak());
                    }
                    view::StepState::Next => {
                        ui.label(egui::RichText::new(head).strong());
                        ui.indent(("job.next", i), |ui| {
                            ui.add(egui::Label::new(egui::RichText::new(&s.says).small()).wrap());
                            if let Some((label, id)) = &s.act {
                                if ui.button(label).clicked() {
                                    act = Some(id);
                                }
                            }
                        });
                    }
                }
            }
        });
        ui.separator();
        if let Some(id) = act {
            self.act(id);
        }
    }

    /// Whether the last reload failed and its note stands (Phase 50 slice 7).
    fn reload_failed(&self) -> bool {
        self.project_note.as_deref().is_some_and(|n| n.starts_with("Not reloaded"))
    }

    /// The review pane (DESIGN.md §5.2): every face once, proposals first,
    /// the residue by row and code; a click selects the row.
    fn review_pane(&mut self, ui: &mut egui::Ui) {
        let mut go: Option<view::Selection> = None;
        // The proposed stock first: the blank before the features.
        match &self.stock_proposal {
            Some(Ok(s)) => {
                ui.label(egui::RichText::new("Proposed stock").strong());
                let head = if s.in_project { format!("{}  · in project", s.head) } else { s.head.clone() };
                if ui.selectable_label(self.selection == view::Selection::ProposedStock, head).clicked() {
                    go = Some(view::Selection::ProposedStock);
                }
            }
            Some(Err(e)) => {
                ui.label(egui::RichText::new("Proposed stock").strong());
                ui.add(egui::Label::new(egui::RichText::new(format!("proposed_stock.part does not parse: {e}")).small()).wrap());
            }
            None => {
                if let Some(why) = &self.stock_declined {
                    ui.label(egui::RichText::new("Proposed stock").strong());
                    ui.add(egui::Label::new(egui::RichText::new(format!("none — {why}; declare the blank by hand (New project…, or the Stock card)")).small()).wrap());
                }
            }
        }
        if let Some(sel) = go {
            self.set_selection(sel);
        }
        match &self.report {
            None => {
                let dir = self.report_path.parent().map(|d| d.display().to_string()).unwrap_or_default();
                ui.label("No report to review");
                ui.add(egui::Label::new(egui::RichText::new(view::import_hint(&dir)).small()).wrap());
                return;
            }
            Some(Err(e)) => {
                ui.label("Report not readable");
                ui.add(egui::Label::new(egui::RichText::new(e).small()).wrap());
                return;
            }
            Some(Ok(r)) => {
                ui.label(egui::RichText::new(format!("{}: {} faces, {} proposed, {} declined", r.source, r.faces_total, r.proposed_count(), r.unsupported_count())).small().weak());
                for line in crate::review::setup_lines(r) {
                    ui.add(egui::Label::new(egui::RichText::new(line).small().weak()).wrap());
                }
            }
        }
        let mut go: Option<usize> = None;
        let mut section = "";
        let pending = view::proposals_waiting(&self.review_rows, &self.review_dups);
        if !self.part.features.is_empty() && ui.small_button("Schedule operations").on_hover_text("Add an operation for every feature that has none, with tools from this project. z undoes.").clicked() {
            self.act("schedule");
        }
        if pending > 0 && ui.small_button(format!("Accept all ({pending})")).on_hover_text("Accept every proposal not yet in the project; duplicates are skipped. z undoes.").clicked() {
            self.act("review.accept_all");
        }
        for (i, row) in self.review_rows.iter().enumerate() {
            if row.section != section {
                section = &row.section;
                ui.add_space(4.0);
                ui.label(egui::RichText::new(section).strong());
            }
            let head = if row.in_project {
                format!("{}  · in project", row.head)
            } else if row.rejected {
                format!("{}  · rejected", row.head)
            } else if let Some(first) = self.review_dups.get(i).copied().flatten() {
                format!("{}  · same as {}", row.head, self.review_rows.get(first).map(|r| r.id.as_str()).unwrap_or("an earlier row"))
            } else {
                row.head.clone()
            };
            if ui.selectable_label(self.selection == view::Selection::Review(i), head).clicked() {
                go = Some(i);
            }
        }
        if let Some(i) = go {
            self.set_selection(view::Selection::Review(i));
        }
    }

    /// A theme picked from Window ▸ Theme (Phase 51 slice 5): `--theme
    /// <name>`'s twin. Resolved the way a launch resolves it, worn at once
    /// where it can be (the fills and the viewport read `self.theme` every
    /// frame), the workspace's `theme` line written NOW so the next launch
    /// wears it, and the glass said honestly: the surface was made
    /// see-through or not when the window opened.
    fn pick_theme(&mut self, choice: Option<String>) {
        self.theme_choice = choice;
        let (theme, note, path) = resolve_theme(&self.theme_choice);
        self.theme_mtime = path.as_deref().and_then(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
        self.theme_path = path;
        self.apply_theme(theme, note);
        if let Some(n) = view::theme_pick_note(self.glass_asked_at_launch, self.theme.background_appearance) {
            self.transient = Some((n, std::time::Instant::now()));
        }
        if let Err(reason) = crate::theme::save_workspace(&self.workspace_snapshot()) {
            startup_say(format!("workspace not saved: {reason}"));
        }
    }

    /// A loaded theme worn: the one seam the launch, the file watch and a
    /// pick all go through — the rows and the marks re-coloured from it.
    fn apply_theme(&mut self, theme: Theme, note: Option<String>) {
        self.theme = theme;
        self.theme_note = note;
        let (rows, annotations) = view::finding_rows(&self.check_result, self.part.stock.z, &self.theme);
        self.rows = rows;
        self.annotations = annotations;
    }

    /// THE selection's one writer (DESIGN.md I6). Its permitted side
    /// effects: a problem that names a place moves the camera there, and —
    /// since Phase 51 slice 3 (§5.4, I14) — a selection with a card OPENS
    /// the form, because a card composed into a shut dock is live and
    /// invisible. Nothing selected opens nothing.
    fn set_selection(&mut self, sel: view::Selection) {
        self.selection = sel;
        if sel != view::Selection::None {
            self.layout.apply(DockAction::OpenForm);
        }
        self.field_edit.clear();
        self.proposal_edges = self.lit_proposal();
        // The one side effect (DESIGN.md §6): a finding's place, and —
        // since 2026-09-17 — an operation's path brought to the centre
        // (`view::frame_point`: the box centre, the scale untouched).
        if let Some(a) = view::selection_anchor(sel, &self.rows, &self.annotations) {
            self.cam.look_at(a);
        } else if let view::Selection::Operation(i) = sel {
            if let Some(c) = view::frame_point(&self.paths, i) {
                self.cam.look_at(c);
            }
        }
    }

    /// The form's card (DESIGN.md §5.4): a pure function of the selection,
    /// rendered — `view::card` decides every word; this only paints.
    fn card_body(&mut self, ui: &mut egui::Ui) {
        // The creation form takes the dock while it is open (DESIGN.md
        // §5.4); Esc dismisses it and its draft.
        if self.new_form.is_some() {
            self.new_feature_form(ui);
            return;
        }
        let card = view::card(&self.part, &self.outline, self.selection, &self.rows, &self.edges, &self.paths, &self.review_rows, self.stock_proposal.as_ref().and_then(|r| r.as_ref().ok()));
        match card {
            view::Card::Nothing => {
                ui.label(egui::RichText::new("Select an operation, a feature, a tool, the stock or a problem — in the sidebar, or an edge in the viewport").weak());
                ui.label(egui::RichText::new("To add one: New feature… and New tool… in the Outline; New operation… on a feature").weak());
            }
            view::Card::Operation { line, removal, tool, form } => {
                ui.label(egui::RichText::new(line).strong());
                // What the op removes and how it cuts are the card's
                // facts, read at body size: they were its smallest text.
                ui.add(egui::Label::new(removal).wrap());
                // The walk as cut, third (ROADMAP item 3 slice 4): a fact
                // about the op, above the fields, where the first picture
                // of the card found it below the fold.
                if let view::Selection::Operation(i) = self.selection {
                    self.walk_card(ui, i);
                    self.fields_form(ui, FieldTarget::Op(i), &form);
                }
                self.tool_card(ui, tool.as_ref());
                if let view::Selection::Operation(i) = self.selection {
                    self.pattern_card(ui, i);
                }
                // The one write (DESIGN.md §1, §5.4): through the one
                // dispatcher, so the button and the palette act are one act.
                ui.horizontal(|ui| {
                    if ui.button("Suppress operation").on_hover_text("Take this operation out of the program but keep its line, commented. z undoes.").clicked() {
                        self.act("op.suppress");
                    }
                    // The tenth write (DESIGN.md §1, §5.4).
                    if ui.button("Delete operation").on_hover_text("Remove this operation's line. z undoes.").clicked() {
                        self.act("statement.delete");
                    }
                });
            }
            view::Card::Feature { name, kind, template, form } => {
                ui.label(format!("{name}  {kind}"));
                // The side, in words (Phase 53 slice 2): which side of the
                // loop the material is — `view::side_word`, for the kinds
                // that have one.
                if let Some(side) = view::side_word(&kind) {
                    ui.label(egui::RichText::new(side).small().weak());
                }
                self.fields_form(ui, FieldTarget::Feature(name.clone()), &form);
                match template {
                    Ok(lines) => {
                        for l in &lines {
                            ui.monospace(l);
                        }
                        // Wrapped, not clipped (ROADMAP item 3 slice 6): at
                        // the opening sidebar width the third button ran
                        // off the card as "Delete featu…".
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Copy template").clicked() {
                                ui.ctx().copy_text(lines.join("\n") + "\n");
                                self.transient = Some(("Template copied".to_string(), std::time::Instant::now()));
                            }
                            // The ninth write's door (DESIGN.md §5.4): the
                            // same lines, as a form.
                            if ui.button("New operation…").on_hover_text("Add an operation on this feature. z undoes.").clicked() {
                                self.act("op.new");
                            }
                            if ui.button("Delete feature").on_hover_text("Remove this feature. Refused while an operation still runs it. z undoes.").clicked() {
                                self.act("statement.delete");
                            }
                            // The side's door (Phase 53 slice 2, fork 1A):
                            // one word and the points the other way, through
                            // the one dispatcher; a kind without a side is
                            // told so on the status line, nothing written.
                            if let Some(to) = view::flipped_kind(&kind) {
                                if ui.button("Flip side").on_hover_text(format!("Make this loop a {to}: the material is on the other side. Its operations stay. z undoes.")).clicked() {
                                    self.act("feature.flip_side");
                                }
                            }
                        });
                    }
                    Err(e) => {
                        ui.label(e);
                    }
                }
            }
            view::Card::Finding { head, text, color, op_line, feature, tool } => {
                ui.label(egui::RichText::new(head).color(rgb(color)).strong());
                ui.add(egui::Label::new(egui::RichText::new(text).small()).wrap());
                if let Some(l) = op_line {
                    ui.label(l);
                }
                if !feature.is_empty() {
                    ui.label(format!("feature  {feature}"));
                }
                self.tool_card(ui, tool.as_ref());
            }
            view::Card::Tool { number, row, body_top, declared, form } => {
                ui.label(row);
                ui.label(if declared {
                    format!("body declared to {}mm above the tip", crate::gcode::f(body_top))
                } else {
                    format!("no body declared — drawn to {}mm above the tip, a presentation height", crate::gcode::f(body_top))
                });
                self.fields_form(ui, FieldTarget::Tool(number), &form);
                if ui.button("Delete tool").on_hover_text("Remove this tool. Refused while an operation still runs it. z undoes.").clicked() {
                    self.act("statement.delete");
                }
            }
            view::Card::Stock { row, form, header } => {
                ui.label(row);
                self.fields_form(ui, FieldTarget::Stock, &form);
                // The header on the stock card (DESIGN.md §5.4, Phase 50
                // slice 3): the project's six words, one form, the line
                // rewritten in place or inserted before the stock.
                ui.add_space(4.0);
                ui.label(egui::RichText::new("Header — the program's words").small().weak());
                self.fields_form(ui, FieldTarget::Header, &header);
            }
            view::Card::Review { head, section, text, statement, refs, in_project, member_of } => {
                ui.label(egui::RichText::new(head).strong());
                ui.label(egui::RichText::new(section).small().weak());
                // The statement is the text Accept will write, edited here in
                // place (ROADMAP item 1, slice 3); the report's own words stay
                // under it. The stock's row and a row in the project read as
                // they are; a residue row has only its reason.
                let (row_id, rejected) = match self.selection {
                    view::Selection::Review(i) => match self.review_rows.get(i) {
                        Some(r) if !r.in_project => (Some(r.id.clone()), r.rejected),
                        Some(r) => (None, r.rejected),
                        None => (None, false),
                    },
                    _ => (None, false),
                };
                match (&statement, &row_id) {
                    (Some(s), Some(id)) => {
                        let draft = self.review_drafts.entry(id.clone()).or_insert_with(|| s.clone());
                        let rows = s.lines().count().clamp(1, 12);
                        ui.add(egui::TextEdit::multiline(draft).code_editor().desired_rows(rows).desired_width(f32::INFINITY).id_salt(("review draft", id)));
                        if draft.trim() != s.trim() {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("edited — Accept writes this text").small());
                                if ui.small_button("Report's text").on_hover_text("Put the report's statement back").clicked() {
                                    *draft = s.clone();
                                }
                            });
                        }
                    }
                    _ => {
                        for line in text.lines() {
                            if statement.as_deref().is_some_and(|s| s.contains(line)) {
                                ui.monospace(line);
                            }
                        }
                    }
                }
                for line in text.lines() {
                    if !statement.as_deref().is_some_and(|s| s.contains(line)) {
                        ui.add(egui::Label::new(egui::RichText::new(line).small()).wrap());
                    }
                }
                if !refs.is_empty() {
                    ui.label(format!("names  {}", refs.join(", ")));
                }
                if let Some(g) = member_of {
                    ui.label(egui::RichText::new(format!("accepting any member accepts {g} once")).small().weak());
                }
                let mut act: Option<&str> = None;
                if statement.is_some() {
                    if in_project {
                        ui.label("In project");
                    } else {
                        ui.horizontal(|ui| {
                            if ui.button("Accept into project").on_hover_text("Add this to the project. z undoes.").clicked() {
                                act = Some("review.accept");
                            }
                            if row_id.is_some() {
                                if rejected {
                                    ui.label("Rejected");
                                    if ui.button("Take the rejection back").on_hover_text("Make this a proposal again. z undoes.").clicked() {
                                        act = Some("review.reject");
                                    }
                                } else if ui.button("Reject").on_hover_text("Mark this proposal rejected, so Accept all skips it. z undoes.").clicked() {
                                    act = Some("review.reject");
                                }
                            }
                        });
                    }
                }
                if let Some(a) = act {
                    self.act(a);
                }
            }
        }
    }

    /// The tool as the card describes it.
    fn tool_card(&mut self, ui: &mut egui::Ui, card: Option<&view::ToolCard>) {
        match card {
            Some(c) => {
                ui.label(format!(
                    "{} — tip at ({}, {}, {})",
                    c.label,
                    crate::gcode::f(c.tip[0]),
                    crate::gcode::f(c.tip[1]),
                    crate::gcode::f(c.tip[2])
                ));
                ui.label(if c.declared {
                    format!("body declared to {}mm above the tip", crate::gcode::f(c.body_top))
                } else {
                    format!("no body declared — drawn to {}mm above the tip, a presentation height", crate::gcode::f(c.body_top))
                });
                self.fields_form(ui, FieldTarget::Tool(c.number), &c.form);
            }
            None => {
                // Under a selected operation this means "no toolpath": a
                // rejected project emits none, and the old sentence told
                // the user to select the operation they had selected.
                ui.label(egui::RichText::new(if self.gcode.is_err() {
                    "No tool position — this project has errors, so no toolpath was generated"
                } else {
                    "No tool position — this operation emitted no moves, or the problem names no tool tip"
                }).weak());
            }
        }
    }

    /// The swept stock surface, built on a worker thread when its layer is
    /// on and the surface in hand is stale (the cursor moved). Progress and
    /// failure report to the status line, never to a pane.
    fn poll_stock_surface(&mut self, ctx: &egui::Context) {
        // Stale when the op cursor OR the block cursor moved (DESIGN.md
        // §5.6, Phase 52 slice 6): the surface follows the move the
        // program names, rebuilt on the worker; one build at a time, so a
        // drag coalesces into the last position.
        if self.sim.is_some() && (self.sim_cursor != self.op_cursor || self.sim_block != self.block_cursor) && self.sim_rx.is_none() {
            self.sim = None;
        }
        if self.sim.is_none() && self.sim_rx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            let part = self.part.clone();
            let emitted_moves = self.emitted_moves.clone();
            let k = self.op_cursor;
            let block = self.block_cursor;
            self.sim_cursor = k;
            self.sim_block = block;
            std::thread::spawn(move || {
                let _ = tx.send(view::stock_surface_prefix(&part, &emitted_moves, k, block, None).map(std::sync::Arc::new));
            });
            self.sim_rx = Some(rx);
        }
        if let Some(rx) = &self.sim_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.sim = Some(result);
                    self.sim_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // The builder panicked — its backtrace is already on
                    // stderr; the status line says so rather than the
                    // picture waiting forever.
                    self.sim = Some(Err(String::from("Simulation failed. See console for details.")));
                    self.sim_rx = None;
                }
            }
        }
    }

    /// The transport bar (DESIGN.md I8, §5.6 as amended 2026-10-18, Phase
    /// 52 slice 13): the cursor's, in two grains and nothing else — row 1
    /// the op cursor (a step back, a step on, the slider, where it stands,
    /// the clock), row 2 the move within the facing operation (`{` `}` as
    /// doors, its slider at the op slider's width, its own words even when
    /// no move is named). Undo and Redo left for the status line's right
    /// end (fork 13a-A). `⏴ ⏵` twice over, told apart by their row: the
    /// fonts have the pair and the glyph lint reads every label.
    fn transport_bar(&mut self, ui: &mut egui::Ui) {
        let n = self.part.operations.len();
        let mut k = self.op_cursor;
        let k_before = self.op_cursor;
        let mut door: Option<&'static str> = None;
        let mut block_pick: Option<usize> = None;
        let blocks = match (&self.gcode, k.checked_sub(1)) {
            (Ok(g), Some(op)) => g.block_count(op),
            _ => 0,
        };
        ui.horizontal(|ui| {
            if ui.small_button("⏴").on_hover_text(view::door_hover("cursor.back", "one operation back")).clicked() {
                k = k.saturating_sub(1);
            }
            if ui.small_button("⏵").on_hover_text(view::door_hover("cursor.on", "one operation on")).clicked() {
                k += 1;
            }
            if n > 0 {
                ui.add(egui::Slider::new(&mut k, 0..=n).integer().show_value(false));
            }
            let t = view::Transport { cursor: k, count: n, block: None, blocks };
            ui.label(t.line(&self.part));
            // The clock (DESIGN.md §5.6, Phase 50 slice 8): the whole
            // program's time, the CLI's own numbers, one weak line.
            ui.label(egui::RichText::new(view::clock_line(&self.clock)).small().weak()).on_hover_text("path length over commanded feed — a floor on reality, not a promise (nacs --time-report)");
        });
        // The move grain's row (DESIGN.md §5.6, Phase 52 slice 13, fork
        // 13c-A): always drawn, its doors disabled and its words saying
        // why when there is nothing to walk. Position 0 of the slider is
        // "no move"; the tool stands at the move's end.
        ui.horizontal(|ui| {
            let can = n > 0;
            if ui.add_enabled(can, egui::Button::new("⏴").small()).on_hover_text(view::door_hover("cursor.block_back", "one move back")).clicked() {
                door = Some("cursor.block_back");
            }
            if ui.add_enabled(can, egui::Button::new("⏵").small()).on_hover_text(view::door_hover("cursor.block_on", "one move on")).clicked() {
                door = Some("cursor.block_on");
            }
            let mut bv = self.block_cursor.map_or(0, |b| b + 1);
            if ui
                .add_enabled(blocks > 0, egui::Slider::new(&mut bv, 0..=blocks.max(1)).integer().show_value(false))
                .on_hover_text(view::MOVE_ROW_HINT)
                .changed()
            {
                block_pick = Some(bv);
            }
            let t = view::Transport { cursor: k_before, count: n, block: self.block_cursor, blocks };
            ui.label(t.move_line());
        });
        self.set_op_cursor(k);
        // The block after the op (the nesting invariant: an op change clears
        // it, so a pick only lands when the op slider stood still).
        if let Some(bv) = block_pick {
            if k == k_before {
                self.block_cursor = bv.checked_sub(1);
                self.scroll_program_to_block = true;
            }
        }
        if let Some(id) = door {
            self.act(id);
        }
    }

    /// The status line (DESIGN.md I10): one line at the bottom, every
    /// background job and every note in one place, empty when nothing is
    /// happening.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let importing = self.import_busy.as_ref().map(|name| format!("Importing {name}…"));
        let running = self.runner_busy.as_ref().map(|e| format!("Running {e}…"));
        let busy = if let Some(note) = importing.as_deref() {
            Some(note)
        } else if let Some(note) = running.as_deref() {
            Some(note)
        } else if self.reload_rx.is_some() {
            Some("Reloading — planning the program…")
        } else if self.sim_rx.is_some() {
            Some("Simulating stock removal…")
        } else if self.cells_rx.is_some() {
            Some("Scanning for uncut material…")
        } else {
            None
        };
        use view::Level;
        let now = std::time::Instant::now();
        let t = self.opened_at.elapsed().as_secs_f64();
        // What the launch said before there was a log to say it in.
        for text in STARTUP.with(|q| std::mem::take(&mut *q.borrow_mut())) {
            self.output.event(t, Level::Info, "window", &text);
        }
        if let Ok(mut q) = WINDOW_SAID.try_lock() {
            for text in q.drain(..) {
                self.output.event(t, Level::Info, "window", &text);
            }
        }
        // An unsent draft is said where the user looks (DESIGN.md §5.7):
        // the draft stands until Enter writes it or Esc drops it. A draft
        // under a HELD grip is not unsent: the hand is on the value and
        // the release writes it. Never logged: it changes with every key.
        let held = self.grip.as_ref().map(|(g, _)| (g.statement.clone(), g.key.clone()));
        let drafts: Vec<(String, String)> = self
            .field_edit
            .iter()
            .filter(|(t, k, _)| held.as_ref().is_none_or(|(ht, hk)| !(t == ht && k == hk)))
            .map(|(_, k, d)| (k.clone(), d.clone()))
            .collect();
        let unsent = view::unsent_notes(&drafts);
        // A transient (a write's confirmation, "Template copied") stands
        // its few seconds and clears on its own.
        if let Some((_, since)) = &self.transient {
            if !view::transient_shown(since.elapsed().as_secs_f64()) {
                self.transient = None;
            }
        }
        // A refusal stands `REFUSAL_SECS` from when its text first
        // appeared, then leaves the line; the log keeps it.
        let mut live = |source: &'static str, text: Option<&str>| -> bool {
            let seen = &mut self.notes_seen;
            let Some(text) = text else {
                seen.retain(|(s, ..)| *s != source);
                return false;
            };
            match seen.iter_mut().find(|(s, ..)| *s == source) {
                Some(e) if e.1 == text => e.2.elapsed().as_secs_f64() < view::REFUSAL_SECS,
                Some(e) => {
                    e.1 = text.to_string();
                    e.2 = now;
                    true
                }
                None => {
                    seen.push((source, text.to_string(), now));
                    true
                }
            }
        };
        if !live("edit", self.edit_note.as_deref()) && self.edit_note.is_some() {
            // Gone from the line, so the same refusal again is a new one.
            self.edit_note = None;
            live("edit", None);
        }
        // A layer that could not be built says so here for a while and
        // beside its own row in the Layers list for as long as it is so.
        let sim_failed = match &self.sim {
            Some(Err(reason)) if self.layout.layers.get(Layer::SimulatedStock) => Some(reason.clone()),
            _ => None,
        };
        let uncut_failed = match &self.cells {
            Some((_, Err(reason))) if self.layout.layers.get(Layer::RestMaterial) => Some(reason.clone()),
            _ => None,
        };
        let sim_shown = live("simulation", sim_failed.as_deref());
        let uncut_shown = live("uncut", uncut_failed.as_deref());
        // The glass (fork 6) is a fact about the launch, not about now:
        // the log has it, the line does not carry it all session.
        let glass_note = view::glass_note(
            self.theme.background_appearance,
            self.glass_granted.lock().ok().and_then(|g| *g),
            &self.blur,
        );
        // Each source posts its own sentence, once (`OutputLog::say`).
        self.output.job(t, busy);
        self.output.say(t, Level::Info, "done", self.transient.as_ref().map_or("", |(s, _)| s.as_str()));
        self.output.say(t, Level::Refused, "edit", self.edit_note.as_deref().unwrap_or(""));
        self.output.say(t, Level::Refused, "run", self.runner_refusal.as_deref().unwrap_or(""));
        self.output.say(t, Level::Refused, "project", self.project_note.as_deref().unwrap_or(""));
        self.output.say(t, Level::Refused, "theme", self.theme_note.as_deref().unwrap_or(""));
        self.output.say(t, Level::Refused, "simulation", sim_failed.as_deref().unwrap_or(""));
        self.output.say(t, Level::Refused, "uncut", uncut_failed.as_deref().unwrap_or(""));
        self.output.say(t, Level::Info, "glass", glass_note.as_deref().unwrap_or(""));
        // The line: what is being typed, what just happened, then what
        // was refused, each refusal in the refusal colour.
        let mut parts: Vec<(&str, Level)> = Vec::new();
        if let Some(b) = busy {
            parts.push((b, Level::Info));
        }
        if let Some(n) = &unsent {
            parts.push((n.as_str(), Level::Info));
        }
        if let Some((text, _)) = &self.transient {
            parts.push((text.as_str(), Level::Info));
        }
        for n in [self.edit_note.as_deref(), self.runner_refusal.as_deref(), self.project_note.as_deref(), self.theme_note.as_deref(), sim_failed.as_deref().filter(|_| sim_shown), uncut_failed.as_deref().filter(|_| uncut_shown)].into_iter().flatten() {
            parts.push((n, Level::Refused));
        }
        let notes: Vec<&str> = parts.iter().skip(usize::from(busy.is_some())).map(|(s, _)| *s).collect();
        let selection = view::selection_sentence(&self.part, &self.outline, self.selection, &self.rows, &self.edges, &self.review_rows);
        let line = view::status_line(&view::StatusParts { busy, notes: &notes });
        let refusal = rgb(self.theme.refusal);
        // Two segments (DESIGN.md §5.7): the notes at the left, the
        // selection at the right, never one string.
        // Legible where it matters: the notes at body size — every refusal
        // and every confirmation lives here — truncated to their share of
        // the line with the whole text on hover, never clipped silently;
        // the selection small at the right, and not `weak` (2.2:1 at 9pt
        // was the review's measurement).
        let mut door: Option<&'static str> = None;
        let (can_undo, can_redo) = (self.edits.to_undo().is_some(), self.edits.to_redo().is_some());
        ui.horizontal(|ui| {
            let total = ui.available_width();
            let height = ui.spacing().interact_size.y;
            // The doors at the right (DESIGN.md §5.7, Phase 50 slice 4; Undo
            // and Redo since Phase 52 slice 13): one click each, then the
            // selection.
            let doors_w = 230.0;
            let left_w = if selection.is_empty() { total - doors_w } else { (total - doors_w) * 0.6 };
            ui.allocate_ui_with_layout(egui::vec2(left_w.max(0.0), height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                // A spinner beside a busy note: a toggle that takes seconds
                // is seen to be working.
                if busy.is_some() {
                    ui.add(egui::Spinner::new().size(12.0));
                }
                let mut job = egui::text::LayoutJob::default();
                let font = egui::TextStyle::Body.resolve(ui.style());
                let plain = ui.visuals().text_color();
                for (i, (text, level)) in parts.iter().enumerate() {
                    if i > 0 {
                        job.append(" · ", 0.0, egui::TextFormat { font_id: font.clone(), color: plain, ..Default::default() });
                    }
                    let color = if *level == Level::Refused { refusal } else { plain };
                    job.append(&text.replace('\n', " "), 0.0, egui::TextFormat { font_id: font.clone(), color, ..Default::default() });
                }
                if parts.is_empty() {
                    job.append(" ", 0.0, egui::TextFormat { font_id: font.clone(), color: plain, ..Default::default() });
                }
                let r = ui.add(egui::Label::new(job).truncate());
                if !line.is_empty() {
                    r.on_hover_text(&line);
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for (id, label, what) in view::STATUS_DOORS.iter().rev() {
                    let enabled = match *id {
                        "edit.undo" => can_undo,
                        "edit.redo" => can_redo,
                        _ => true,
                    };
                    if ui.add_enabled(enabled, egui::Button::new(*label).small()).on_hover_text(view::door_hover(id, what)).clicked() {
                        door = Some(*id);
                    }
                }
                if !selection.is_empty() {
                    let sel = selection.replace('\n', " ");
                    ui.add(egui::Label::new(egui::RichText::new(&sel).small()).truncate()).on_hover_text(&sel);
                }
            });
        });
        if let Some(id) = door {
            self.act(id);
        }
    }

    fn viewport(&mut self, ui: &mut egui::Ui) {
        let layers = self.layout.layers;
        // Everything the picture lights derives from the one selection.
        let sel_op = view::selected_op(self.selection, &self.rows);
        let sel_feature = view::selected_feature(self.selection, &self.rows, &self.edges);
        // The block cursor's move first (DESIGN.md §5.6, Phase 52 slice 5),
        // the deepest point otherwise — both `view`'s, the shell choosing
        // nothing but the order.
        let pose = view::block_pose(&self.part, &self.emitted_moves, self.op_cursor, self.block_cursor)
            .or_else(|| view::tool_pose(&self.part, self.selection, &self.rows, &self.paths));
        self.pose_drawn = pose;
        if layers.simulated_stock {
            self.poll_stock_surface(ui.ctx());
        }
        if layers.rest_material {
            self.poll_rest_material(ui.ctx());
        }
        let (resp, painter) =
            ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = resp.rect;
        self.vp_size = (rect.width(), rect.height());

        // The map's inset (DESIGN.md I2): always drawn when the viewport can
        // hold it, `None` when it cannot — and then the viewport keeps every
        // gesture.
        let inset = view::map_inset_rect(rect.width(), rect.height());

        // A gesture belongs to whatever it STARTED on, decided ONCE from
        // `press_origin` at the frame it begins (`view::map_owns_gesture`,
        // where the pin reaches it) and kept until every button is up —
        // egui's origin is per-pointer, so a chorded second press
        // mid-gesture would otherwise re-aim a running drag.
        let any_down = ui.input(|i| {
            [egui::PointerButton::Primary, egui::PointerButton::Secondary, egui::PointerButton::Middle]
                .iter()
                .any(|b| i.pointer.button_down(*b))
        });
        if any_down {
            if self.map_gesture.is_none() {
                if let Some(o) = ui.input(|i| i.pointer.press_origin()) {
                    self.map_gesture =
                        Some(view::map_owns_gesture(inset, o.x - rect.left(), o.y - rect.top()));
                }
            }
        } else {
            self.map_gesture = None;
        }
        let steering = self.map_gesture == Some(true);

        // The handles (Phase 41 slice 4): a press on a grip takes the
        // gesture; the drag drafts the value through `view::handle_drag`
        // and the draft is shown in the form as any typed draft is; the
        // release writes it through `set_field` — the one write path.
        // The model is `view::handles`; this only reports the pointer.
        let handles = view::handles(&self.part, self.selection, &self.rows, &self.edges);
        let centre = rect.center();
        let to_view = |pos: egui::Pos2| (f64::from(pos.x - centre.x), f64::from(centre.y - pos.y));
        let primary_down = ui.input(|i| i.pointer.button_down(egui::PointerButton::Primary));
        if self.grip.is_none() && primary_down && !steering {
            if let Some(o) = ui.input(|i| i.pointer.press_origin()) {
                if rect.contains(o) {
                    let (sx, sy) = to_view(o);
                    if let Some(k) = view::pick_handle(&handles, &self.cam, sx, sy, PICK_TOL) {
                        self.grip = Some((handles[k].clone(), (sx, sy)));
                        self.tween = None;
                    }
                }
            }
        }
        let mut grip_took_click = false;
        if let Some((h, from)) = self.grip.clone() {
            grip_took_click = true;
            let value = ui.input(|i| i.pointer.latest_pos()).and_then(|pos| view::handle_drag(&h, &self.cam, from, to_view(pos)));
            // A drag at its grip's ceiling says so on the status line, once
            // per frame it is held there (the transient's own clock).
            if let Some(stop) = value.and_then(|v| view::handle_stop(&h, v)) {
                self.transient = Some((stop.to_string(), std::time::Instant::now()));
            }
            let draft = value.map(view::handle_text);
            if primary_down {
                if let Some(text) = draft {
                    self.field_edit.retain(|(t, k, _)| !(*t == h.statement && *k == h.key));
                    self.field_edit.push((h.statement.clone(), h.key.clone(), text));
                }
            } else {
                self.grip = None;
                let drafted = self.field_edit.iter().find(|(t, k, _)| *t == h.statement && *k == h.key).map(|(_, _, d)| d.clone());
                match drafted {
                    Some(text) if text != view::handle_text(h.value) => self.set_field(h.statement.clone(), &h.key, Some(&text)),
                    _ => self.field_edit.retain(|(t, k, _)| !(*t == h.statement && *k == h.key)),
                }
            }
        }

        if !self.fitted {
            self.cam.fit_to_aabb(self.fit_bb, rect.width(), rect.height());
            self.fitted = true;
        }

        // The focus (Phase 41 slice 1): the goal camera is
        // `Camera::focused_on` the selection's box (`view::focus_aabb`),
        // or the scene's when nothing selected has one; the move to it is
        // a tween the shell only STEPS — `Camera::toward` at the eased
        // fraction of `FOCUS_SECS`, where the pins reach it.
        if self.focus_pending {
            self.focus_pending = false;
            let bb = view::focus_aabb(&self.part, self.selection, &self.rows, &self.edges, &self.paths).unwrap_or(self.fit_bb);
            let goal = self.cam.focused_on(bb, rect.width(), rect.height());
            self.tween = Some((self.cam, goal, std::time::Instant::now()));
        }
        if let Some((from, to, t0)) = self.tween {
            let t = t0.elapsed().as_secs_f64() / view::FOCUS_SECS;
            self.cam = from.toward(&to, view::ease(t));
            if t >= 1.0 {
                self.tween = None;
            } else {
                ui.ctx().request_repaint();
            }
        }

        // Input → camera: left-drag orbits, right-drag pans, scroll zooms.
        // Any button still held means the user is mid-gesture, whatever
        // egui decided to call this frame's events.
        let gesture_in_progress = ui.input(|i| {
            [egui::PointerButton::Primary, egui::PointerButton::Secondary, egui::PointerButton::Middle]
                .iter()
                .any(|b| i.pointer.button_down(*b))
        });
        if let (true, Some(r)) = (steering, inset) {
            // The gesture is the map's: steer the viewport to where it
            // points, and the orbit, the pan and the pick below never
            // see it. Clamped into the inset, so a drag that leaves the
            // corner keeps steering from its edge rather than jumping.
            if resp.clicked() || resp.dragged() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let map_rect = egui::Rect::from_min_size(
                        egui::pos2(rect.left() + r[0], rect.top() + r[1]),
                        egui::vec2(r[2], r[3]),
                    );
                    let map_cam = view::Camera::plan_fit(self.fit_bb, r[2], r[3]);
                    let pos = map_rect.clamp(pos);
                    let c = map_rect.center();
                    let world =
                        map_cam.unproject_screen(f64::from(pos.x - c.x), f64::from(c.y - pos.y));
                    self.cam.target[0] = world[0];
                    self.cam.target[1] = world[1];
                }
            }
        } else if grip_took_click {
            // The press was a grip's: no pick, no orbit.
        } else if resp.clicked() && !gesture_in_progress {
            // Click a face: the nearest edge within the
            // pick tolerance selects its feature — the hit test is
            // `view::pick_feature`, where tests reach it; this shell only
            // converts the pointer to camera screen coordinates. A miss
            // clears the pick — clicking empty space is how you put the
            // template panel away.
            //
            // Guarded by `gesture_in_progress`: egui promotes a chorded tap
            // (a quick primary press-release during a right-button pan) to
            // a CLICK, because any press resets the click bookkeeping and a
            // click event suppresses `is_decidedly_dragging`. Unguarded,
            // that stray emitted moves would silently re-aim the selection mid-pan.
            if let Some(pos) = resp.interact_pointer_pos() {
                let c = rect.center();
                let hit = view::pick_feature(
                    &self.edges,
                    &self.cam,
                    f64::from(pos.x - c.x),
                    f64::from(c.y - pos.y),
                    PICK_TOL,
                );
                self.set_selection(match hit {
                    Some(i) => view::Selection::Feature(i),
                    None => view::Selection::None,
                });
            }
        } else {
            if resp.dragged_by(egui::PointerButton::Primary) && self.grip.is_none() {
                // A hand on the camera ends a focus in flight: the move
                // was to help, and the user has taken over.
                self.tween = None;
                let d = resp.drag_delta();
                self.cam.yaw -= f64::from(d.x) * 0.01;
                self.cam.pitch = (self.cam.pitch + f64::from(d.y) * 0.01)
                    .clamp(-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
            }
            if resp.dragged_by(egui::PointerButton::Secondary)
                || resp.dragged_by(egui::PointerButton::Middle)
            {
                self.tween = None;
                let d = resp.drag_delta();
                let (r, u) = (self.cam.right(), self.cam.up());
                let (dx, dy) = (f64::from(d.x) / self.cam.scale, f64::from(d.y) / self.cam.scale);
                for k in 0..3 {
                    // Screen y is down; camera up is up — hence the sign split.
                    self.cam.target[k] -= r[k] * dx - u[k] * dy;
                }
            }
        }
        // The hover says what a click would pick (DESIGN.md §5.5, Phase 50
        // slice 9): the same test the click runs, never during a grip or a
        // gesture; the pointing hand says it is pickable.
        self.hover_feature = None;
        if resp.hovered() && self.grip.is_none() && !gesture_in_progress {
            if let Some(pos) = resp.hover_pos() {
                let c = rect.center();
                self.hover_feature = view::pick_feature(&self.edges, &self.cam, f64::from(pos.x - c.x), f64::from(c.y - pos.y), PICK_TOL);
                if self.hover_feature.is_some() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            }
        }
        // The viewport's own menu (DESIGN.md §5.5, Phase 50 slice 9): a
        // right-CLICK — a press with no drag; a right-drag is still the pan
        // — picks what is under it and offers its acts; a double-click
        // focuses the pick.
        if resp.secondary_clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let c = rect.center();
                let hit = view::pick_feature(&self.edges, &self.cam, f64::from(pos.x - c.x), f64::from(c.y - pos.y), PICK_TOL);
                self.set_selection(hit.map_or(view::Selection::None, view::Selection::Feature));
            }
        }
        if resp.double_clicked() {
            self.act("view.focus");
        }
        let mut viewport_act: Option<&'static str> = None;
        let menu_sel = self.selection;
        resp.context_menu(|ui| {
            for (id, label) in view::context_actions(menu_sel) {
                if ui.button(label).clicked() {
                    viewport_act = Some(id);
                    ui.close();
                }
            }
        });
        if let Some(id) = viewport_act {
            self.act(id);
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.tween = None;
                // About the cursor (DESIGN.md §5.5, 2026-09-17): the point
                // under the pointer stays under it — `Camera::zoom_at`,
                // clamped there as `zoom_by` is. No pointer, the centre.
                let factor = f64::from(scroll * 0.002).exp();
                let c = rect.center();
                match resp.hover_pos() {
                    Some(pos) => self.cam.zoom_at(f64::from(pos.x - c.x), f64::from(c.y - pos.y), factor),
                    None => self.cam.zoom_by(factor),
                }
            }
        }

        // A dark ground for the machinist's view — the theme's `ground`
        // slot: every colour drawn on it is chosen against it, and pinned
        // legible there.
        painter.rect_filled(rect, 0.0, rgba(self.theme.ground, self.theme.fill_alpha.ground));

        let center = rect.center();
        // One stance for the frame — every edge and toolpath point below goes
        // through it (`view::Camera::lens`).
        let lens = self.cam.lens();
        let to_screen = |p: [f64; 3]| -> egui::Pos2 {
            let (x, y, _) = lens.project(p);
            egui::pos2(center.x + x, center.y - y) // +y up → screen y down
        };

        // The stock: 12 edges, the quiet reference everything sits in.
        let bb = view::stock_aabb(&self.part.stock);
        let corner = |k: usize| -> egui::Pos2 {
            to_screen([
                bb[if k & 1 == 0 { 0 } else { 3 }],
                bb[if k & 2 == 0 { 1 } else { 4 }],
                bb[if k & 4 == 0 { 2 } else { 5 }],
            ])
        };
        let stock_stroke = egui::Stroke::new(1.0, rgb(self.theme.stock));
        for (a, b) in [
            (0, 1), (2, 3), (4, 5), (6, 7), // X edges
            (0, 2), (1, 3), (4, 6), (5, 7), // Y edges
            (0, 4), (1, 5), (2, 6), (3, 7), // Z edges
        ] {
            painter.line_segment([corner(a), corner(b)], stock_stroke);
        }

        // The part: every declared feature's wireframe, the quiet layer
        // between the stock and the toolpaths. The PICKED one is held back
        // to after the sim surface — an opaque surface painted over it
        // would hide the gold lift exactly when the sim is on, and the
        // pick is a selection state, not a thing to be occluded.
        let edge_stroke = egui::Stroke::new(1.0, rgb(self.theme.wire));
        let picked_stroke = egui::Stroke::new(1.5, rgb(self.theme.select));
        let draw_edge = |edge: &FeatureEdges, stroke: egui::Stroke| {
            for pl in &edge.polylines {
                let pts: Vec<egui::Pos2> = pl.iter().map(|p| to_screen(*p)).collect();
                if pts.len() >= 2 {
                    painter.add(egui::Shape::line(pts, stroke));
                }
            }
        };
        for (wi, edge) in self.edges.iter().enumerate() {
            if layers.outlines && sel_feature != Some(wi) {
                draw_edge(edge, edge_stroke);
            }
        }

        // The machined stock: an isosurface through the sim pipeline, whose
        // DEPTH BUFFER does the occluding. The heightfield that stood here
        // got its ordering from the upward-closure lemma — every XY column
        // one interval, so painter's order sufficed — and gave up every
        // undercut to have it. Over the edges, under the toolpaths: the
        // surface is what the edges promised, and the paths stay the loudest
        // layer (they are painted after, and are not depth-tested against
        // the stock, which is a decision and not an oversight).
        if layers.simulated_stock {
            if let Some(Ok(surface)) = &self.sim {
                // ONE MESH, in depth order — not a shape per triangle.
                //
                // TRIANGLES, not quads: a heightfield cell whose corners
                // straddle a wall is non-planar, and its projection can be
                // non-convex (a bowtie at an oblique orbit), which a convex
                // fill would paint outside the true silhouette — at exactly
                // the walls the shading exists to make legible. A triangle
                // is convex under any projection.
                //
                // But a triangle is not safe to STROKE, and each one used to
                // carry a hairline of its own colour to close the seam that
                // a per-shape fill feathers. A heightfield projects a few
                // near-degenerate slivers at any given camera — two corners
                // landing a thousandth of a pixel apart is an ordinary,
                // CORRECT projection — and stroking a closed path with a
                // 180-degree hairpin sends the miter join to infinity. Two
                // such triangles out of a hundred thousand put vertices
                // 29,784 px from the centre of an 875 px viewport: the
                // slivers that ran off the frame in every direction,
                // measured by tessellating these very shapes.
                //
                // A raw mesh has no stroke to blow up and no per-shape
                // feathering to seam, so the hairline has nothing left to
                // fix — the triangles share their corner vertices exactly.
                // It is also one shape where there were 105,876.
                //
                // `base` is the running vertex count and MUST be added to
                // every index: the one real hazard of concatenating into a
                // shared buffer, which is why the indices are written
                // relative to it and never to zero.
                let radius = surface.depth_radius(self.cam.target);
                let clip = self.cam.lens().clip_matrix(
                    f64::from(rect.width()),
                    f64::from(rect.height()),
                    radius,
                );
                painter.add(eframe::egui_wgpu::Callback::new_paint_callback(
                    rect,
                    SimCallback { clip, iso: surface.clone(), base: self.theme.sim_base },
                ));
            }
        }

        // The immersion field (Phase 41 slice 2): one filled square per
        // cell at the floor's height, in the engagement family's colour at
        // three alphas — advisory-versus-error is a brightness, never a
        // hue, the house's own rule for the families. The model is
        // `engagement::immersion`; built once per op and kept until the
        // project changes; a decline is a status note, not a blank.
        if layers.engagement {
            let op = view::region_op(&self.part, self.selection, &self.rows, &self.edges);
            match (op, &self.immersion) {
                (Some(i), Some((k, _))) if *k == i => {}
                (Some(i), _) => self.immersion = Some((i, crate::engagement::immersion(&self.part, i))),
                (None, _) => self.immersion = None,
            }
            // One mesh for the whole field, its squares within a budget
            // (ROADMAP item 3 slice 3): a polygon per cell per frame was
            // 101 ms a frame on the plate's face under a ⌀2, measured
            // headless in a debug build; the layer off, 9 ms.
            if let Some((k, Ok(f))) = &self.immersion {
                if !self.immersion_blocks.as_ref().is_some_and(|(j, _)| j == k) {
                    self.immersion_blocks = Some((*k, view::immersion_blocks(f)));
                }
            }
            if let Some((_, b)) = &self.immersion_blocks {
                let half = b.side * 0.5;
                let mut mesh = egui::Mesh::default();
                mesh.reserve_vertices(b.blocks.len() * 4);
                mesh.reserve_triangles(b.blocks.len() * 2);
                for (x, y, band) in &b.blocks {
                    let alpha = match band {
                        crate::engagement::Band::Within => 45,
                        crate::engagement::Band::Above => 120,
                        crate::engagement::Band::Past90 => 210,
                    };
                    let color = rgba(self.theme.family_engagement, alpha);
                    let i = mesh.vertices.len() as u32;
                    mesh.colored_vertex(to_screen([x - half, y - half, b.z]), color);
                    mesh.colored_vertex(to_screen([x + half, y - half, b.z]), color);
                    mesh.colored_vertex(to_screen([x + half, y + half, b.z]), color);
                    mesh.colored_vertex(to_screen([x - half, y + half, b.z]), color);
                    mesh.add_triangle(i, i + 1, i + 2);
                    mesh.add_triangle(i, i + 2, i + 3);
                }
                painter.add(egui::Shape::mesh(mesh));
            }
        }

        // The hover's candidate, dimmer (Phase 50 slice 9), then the picked
        // feature over the surface: the selection reads through.
        if let Some(edge) = self.hover_feature.filter(|h| Some(*h) != sel_feature).and_then(|wi| self.edges.get(wi)) {
            draw_edge(edge, egui::Stroke::new(1.5, rgba(self.theme.select, 110)));
        }
        if let Some(edge) = sel_feature.and_then(|wi| self.edges.get(wi)) {
            draw_edge(edge, picked_stroke);
        }
        // The floor's kinds (Phase 41 slice 1): what every stretch of the
        // selected floor's boundary is to the tool, told apart by STROKE —
        // solid where material stands, dashed where there is air, dotted
        // over a bridged mouth — in the `removal` slot, because the floor
        // an op works IS the declared removal's boundary. The model is
        // `view::region_strokes`; this only paints. Rides the Outlines
        // layer, as the edges it annotates do.
        if layers.outlines {
            if let Some(op) = view::region_op(&self.part, self.selection, &self.rows, &self.edges) {
                let stroke = egui::Stroke::new(2.5, rgb(self.theme.removal));
                for ks in view::region_strokes(&self.part, op, &self.edges) {
                    let pts: Vec<egui::Pos2> = ks.pts.iter().map(|p| to_screen(*p)).collect();
                    if pts.len() < 2 {
                        continue;
                    }
                    match ks.kind {
                        view::EdgeStroke::Wall => painter.add(egui::Shape::line(pts, stroke)),
                        view::EdgeStroke::Open => painter.add(egui::Shape::dashed_line(&pts, stroke, 8.0, 6.0)),
                        view::EdgeStroke::Bridged => painter.add(egui::Shape::dashed_line(&pts, stroke, 2.0, 4.0)),
                    };
                }
            }
        }
        // The grips (Phase 41 slice 4): a small square at each handle in
        // the selection colour, its key beside it; the held one larger.
        let mut grips_drawn: Vec<(String, egui::Pos2)> = Vec::new();
        for h in &handles {
            // Painted where `pick_handle` picks: the one home for a grip's
            // place on screen (a standing-off grip is not at its point).
            let Some((vx, vy)) = view::handle_view_point(h, &self.cam) else { continue };
            let c = egui::pos2(centre.x + vx as f32, centre.y - vy as f32);
            grips_drawn.push((h.key.clone(), c));
            let held = self.grip.as_ref().is_some_and(|(g, _)| g.statement == h.statement && g.key == h.key);
            let half = if held { 5.0 } else { 3.5 };
            painter.rect_filled(egui::Rect::from_center_size(c, egui::vec2(half * 2.0, half * 2.0)), 1.0, rgb(self.theme.select));
            painter.text(
                c + egui::vec2(half + 3.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &h.key,
                egui::FontId::proportional(10.0),
                rgb(self.theme.text),
            );
        }
        self.grips_drawn = grips_drawn;
        // The selected proposal's own edges (DESIGN.md §5.2): what the
        // review lights — the statement, never the STEP face.
        if let Some(edges) = &self.proposal_edges {
            for edge in edges {
                draw_edge(edge, picked_stroke);
            }
        }

        // The declared stock's boundaries at the cursor, under
        // the toolpaths: the moat, the band, the tube, the island as edges.
        if let (true, Ok(edges)) = (layers.declared_stock, &self.removal.1) {
            let stroke = egui::Stroke::new(1.0, rgba(self.theme.removal, 150));
            for w in edges {
                let pts: Vec<egui::Pos2> = w.iter().map(|p| to_screen(*p)).collect();
                if pts.len() >= 2 {
                    painter.add(egui::Shape::line(pts, stroke));
                }
            }
        }
        // The fixture's keep-outs, and the tool where it
        // stands — over the stock edges, under the toolpaths and annotations.
        {
            let fx = egui::Stroke::new(1.0, rgba(self.theme.fixture, 200));
            for w in self.fixture.iter().filter(|_| layers.fixtures) {
                let pts: Vec<egui::Pos2> = w.iter().map(|p| to_screen(*p)).collect();
                if pts.len() >= 2 {
                    painter.add(egui::Shape::line(pts, fx));
                }
            }
            if let (true, Some((tip, t))) = (layers.tool, pose) {
                if let Some(tool) = self.part.tools.get(&t) {
                    let sil = view::Silhouette::of(tool);
                    let ts = egui::Stroke::new(1.0, rgba(self.theme.tool, 220));
                    for w in view::tool_body(&sil, tip) {
                        let pts: Vec<egui::Pos2> = w.iter().map(|p| to_screen(*p)).collect();
                        if pts.len() >= 2 {
                            painter.add(egui::Shape::line(pts, ts));
                        }
                    }
                }
            }
        }
        // The rest cells at the cursor: one dot per cell, two colours.
        if layers.rest_material {
            if let Some((_, Ok(c))) = &self.cells {
                let r = (self.cam.scale * c.cell * 0.5).clamp(1.0, 4.0) as f32;
                for (cells, col) in [(&c.forgotten, self.theme.forgotten), (&c.violated, self.theme.violated)] {
                    let color = rgba(col, 200);
                    for p in cells.iter() {
                        painter.circle_filled(to_screen(*p), r, color);
                    }
                }
            }
        }

        // The toolpaths through the cursor: role-colored feed motion, dim
        // dashed rapids, the selected op lifted with the highlight color and
        // a heavier stroke. Ops past `k` have not run yet and do not draw.
        for path in self.paths.iter().filter(|p| layers.toolpaths && p.op_idx < self.op_cursor) {
            let selected = sel_op == Some(path.op_idx);
            let dimmed = sel_op.is_some() && !selected;
            let (feed_rgb, alpha) = view::feed_paint(&self.theme, path.role, selected, dimmed);
            let color = rgba(feed_rgb, alpha);
            let rapid_color = rgba(self.theme.rapid, if dimmed { 40 } else { 160 });

            for pl in &path.polylines {
                let pts: Vec<egui::Pos2> = pl.pts.iter().map(|p| to_screen(*p)).collect();
                if pts.len() < 2 {
                    continue;
                }
                if pl.rapid {
                    painter.extend(egui::Shape::dashed_line(
                        &pts,
                        egui::Stroke::new(1.0, rapid_color),
                        4.0,
                        4.0,
                    ));
                } else {
                    let w = if selected { 2.5 } else { 1.5 };
                    painter.add(egui::Shape::line(pts, egui::Stroke::new(w, color)));
                }
            }
        }

        // The walk on the selected path (ROADMAP item 3 slice 4): the
        // stretches past half immersion and the slots, in the engagement
        // family's colour at two brightnesses, over the path they belong
        // to; each entry a dot where the descent arrived. The model is
        // `compare::walk_as_emitted`, measured on the card's worker.
        if let (Some(i), true) = (sel_op, layers.toolpaths) {
            if let Some((k, _, Some(w))) = &self.walk {
                if *k == i {
                    for s in &w.stretches {
                        let (width, alpha) = match s.kind {
                            crate::compare::Station::Slot => (4.0, 255),
                            crate::compare::Station::PastHalf => (3.0, 170),
                            _ => continue,
                        };
                        let pts: Vec<egui::Pos2> = s.pts.iter().map(|p| to_screen([p.x, p.y, w.z])).collect();
                        painter.add(egui::Shape::line(pts, egui::Stroke::new(width, rgba(self.theme.family_engagement, alpha))));
                    }
                    for (x, y, z) in &w.entries {
                        painter.circle_filled(to_screen([*x, *y, *z]), 4.0, rgba(self.theme.family_engagement, 255));
                    }
                }
            }
        }

        // The annotations — facts with a place, after the toolpaths so they are
        // never under them: a cross at a point, a line along a segment, the
        // label beside. What an annotation MEANS is its colour slot and its label,
        // both chosen by the view model that made it (`view::Annotation`).
        for annotation in self.annotations.iter().filter(|_| layers.annotations) {
            let color = rgb(annotation.color);
            let stroke = egui::Stroke::new(1.5, color);
            let anchor = match &annotation.shape {
                view::AnnotationShape::Point(p) => {
                    let c = to_screen(*p);
                    painter.line_segment(
                        [egui::pos2(c.x - ANNOTATION_HALF, c.y), egui::pos2(c.x + ANNOTATION_HALF, c.y)],
                        stroke,
                    );
                    painter.line_segment(
                        [egui::pos2(c.x, c.y - ANNOTATION_HALF), egui::pos2(c.x, c.y + ANNOTATION_HALF)],
                        stroke,
                    );
                    c
                }
                view::AnnotationShape::Segment(a, b) => {
                    let (sa, sb) = (to_screen(*a), to_screen(*b));
                    painter.line_segment([sa, sb], stroke);
                    to_screen(annotation.anchor())
                }
            };
            if !annotation.label.is_empty() {
                painter.text(
                    egui::pos2(anchor.x + ANNOTATION_LABEL_DX, anchor.y),
                    egui::Align2::LEFT_CENTER,
                    &annotation.label,
                    egui::FontId::proportional(12.0),
                    color,
                );
            }
        }

        // The map's inset, LAST — it floats over the viewport, so it is
        // drawn over everything the viewport drew. Same `minimap` the dock
        // one map, one place, and no second renderer.
        if let Some(r) = inset {
            let map_rect = egui::Rect::from_min_size(
                egui::pos2(rect.left() + r[0], rect.top() + r[1]),
                egui::vec2(r[2], r[3]),
            );
            let map_cam = view::Camera::plan_fit(self.fit_bb, r[2], r[3]);
            self.minimap(&painter, map_rect, &map_cam);
            self.triad(&painter, map_rect);
            // A border, so the inset reads as a thing lying ON the
            // viewport rather than a hole in it.
            painter.rect_stroke(
                map_rect,
                2.0,
                egui::Stroke::new(1.0, rgb(self.theme.border)),
                egui::StrokeKind::Inside,
            );
        }

        // The empty viewport says where the first feature or operation
        // comes from (DESIGN.md §5.5, 2026-09-17) — `view::empty_viewport_note`.
        // The two stances without a program (DESIGN.md §1, Phase 50 slice
        // 3) speak first: no main.part, or a file that does not parse.
        let stance = view::stance_note(&self.part, self.check_result.refusal.as_deref());
        if let Some(note) = stance.as_deref().or_else(|| view::empty_viewport_note(self.part.operations.len(), self.part.features.len())) {
            painter.text(center, egui::Align2::CENTER_CENTER, note, egui::FontId::proportional(14.0), rgb(self.theme.text));
        }

        // The Layers list and the view strip live in the band's row over
        // the viewport since Phase 52 slice 2 (`viewport_row`); the map
        // is the one overlay (I2).
    }

    /// The axis triad in the map's corner (DESIGN.md §5.5, 2026-09-17): the
    /// MAIN camera's X, Y and Z as `Camera::triad` projects them — the map
    /// is the orientation home (I2), and this says which way the picture's
    /// axes run. An axis looking at the viewer is a dot.
    fn triad(&self, painter: &egui::Painter, map_rect: egui::Rect) {
        const ARM: f32 = 16.0;
        let origin = map_rect.left_bottom() + egui::vec2(ARM + 6.0, -(ARM + 6.0));
        for (dx, dy, name) in self.cam.triad() {
            let tip = egui::pos2(origin.x + dx * ARM, origin.y - dy * ARM);
            let stroke = egui::Stroke::new(1.5, rgb(self.theme.text));
            if (dx * dx + dy * dy).sqrt() < 0.05 {
                painter.circle_filled(origin, 2.0, stroke.color);
            } else {
                painter.line_segment([origin, tip], stroke);
            }
            painter.text(
                egui::pos2(tip.x + dx * 6.0, tip.y - dy * 6.0),
                egui::Align2::CENTER_CENTER,
                name,
                egui::FontId::proportional(10.0),
                stroke.color,
            );
        }
    }


    /// The map: the whole
    /// project in the drawing-sheet plan, colors keyed to the derived role
    /// through the same `theme::Theme::role_color` home the main view reads. Feed
    /// motion only — at map scale the rapids read as noise over the cutting
    /// story, and the main view remains the record that draws them. The
    /// bright quad is the main camera's footprint (`view::viewport_footprint`),
    /// clipped with everything else to the map's rect.
    fn minimap(&self, painter: &egui::Painter, map_rect: egui::Rect, map_cam: &view::Camera) {
        let sel_op = view::selected_op(self.selection, &self.rows);
        let p = painter.with_clip_rect(map_rect);
        p.rect_filled(map_rect, 2.0, rgba(self.theme.map_ground, self.theme.fill_alpha.map_ground));

        let c = map_rect.center();
        let map_lens = map_cam.lens();
        let to_map = |pt: [f64; 3]| -> egui::Pos2 {
            let (x, y, _) = map_lens.project(pt);
            egui::pos2(c.x + x, c.y - y)
        };

        // The stock's footprint — the frame everything else sits in.
        let bb = view::stock_aabb(&self.part.stock);
        let corners = vec![
            to_map([bb[0], bb[1], bb[5]]),
            to_map([bb[3], bb[1], bb[5]]),
            to_map([bb[3], bb[4], bb[5]]),
            to_map([bb[0], bb[4], bb[5]]),
        ];
        let frame_stroke = egui::Stroke::new(1.0, rgb(self.theme.stock));
        p.add(egui::Shape::closed_line(corners, frame_stroke));

        // The part, quiet under the paths — same layer order as the main view.
        let edge_color = rgba(self.theme.wire, 110);
        for edge in &self.edges {
            for pl in &edge.polylines {
                let pts: Vec<egui::Pos2> = pl.iter().map(|pt| to_map(*pt)).collect();
                if pts.len() >= 2 {
                    p.add(egui::Shape::line(pts, egui::Stroke::new(1.0, edge_color)));
                }
            }
        }

        // The cutting motion, role-colored — the map's whole point. The
        // selection reads the same way as the main view: lifted, others dim.
        for path in &self.paths {
            let selected = sel_op == Some(path.op_idx);
            let dimmed = sel_op.is_some() && !selected;
            let (feed_rgb, alpha) = view::feed_paint(&self.theme, path.role, selected, dimmed);
            let color = rgba(feed_rgb, alpha);
            for pl in path.polylines.iter().filter(|pl| !pl.rapid) {
                let pts: Vec<egui::Pos2> = pl.pts.iter().map(|pt| to_map(*pt)).collect();
                if pts.len() >= 2 {
                    p.add(egui::Shape::line(pts, egui::Stroke::new(1.0, color)));
                }
            }
        }

        // Where the main camera looks, clipped to the stock
        // (`view::viewport_indicator` owns the reasoning): no indicator at
        // all when the view already holds the whole part, because a bright
        // quad slicing over the map says "cropped" at the one moment nothing
        // is. Still collapses to a segment on an edge-on view — honest, and
        // exactly what the footprint of an edge-on view is.
        if let Some(fp) = (self.vp_size.0 > 0.0)
            .then(|| view::viewport_indicator(&self.cam, self.vp_size.0, self.vp_size.1, bb))
            .flatten()
        {
            let quad: Vec<egui::Pos2> = fp.iter().map(|pt| to_map(*pt)).collect();
            let (ind, alpha) = self.theme.indicator;
            let indicator = egui::Stroke::new(1.0, rgba(ind, alpha));
            p.add(egui::Shape::closed_line(quad, indicator));
        }

        p.rect_stroke(map_rect, 2.0, frame_stroke, egui::StrokeKind::Inside);
    }
}

/// The points a proposed stock carries: its lines that are neither the
/// statement, its closing paren nor a comment.
fn proposal_points(proposal: &str) -> usize {
    proposal
        .lines()
        .filter(|l| {
            let c = l.split('#').next().unwrap_or("").trim();
            !c.is_empty() && !c.starts_with("stock ") && c != ")"
        })
        .count()
}
