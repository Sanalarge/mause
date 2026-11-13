//! A short-lived, input-transparent layer-shell surface, above fullscreen windows.
use std::io::Write;
use std::os::fd::AsFd;
use std::time::Duration;
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_output, wl_region, wl_registry, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols::xdg::xdg_output::zv1::client::{zxdg_output_manager_v1, zxdg_output_v1};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, Layer},
    zwlr_layer_surface_v1::{self, Anchor, KeyboardInteractivity},
};

#[derive(Default)]
struct State {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    output_manager: Option<zxdg_output_manager_v1::ZxdgOutputManagerV1>,
    outputs: Vec<Output>,
    configured: bool,
    closed: bool,
}

struct Output {
    output: wl_output::WlOutput,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

pub(super) fn show(x: i32, y: i32, w: i32, h: i32) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    let mut state = State::default();
    let _registry = conn.display().get_registry(&qh, ());
    queue.roundtrip(&mut state)?;
    let shell = state.shell.as_ref().ok_or("layer-shell unavailable")?;
    let compositor = state.compositor.as_ref().ok_or("compositor unavailable")?;
    let shm = state.shm.as_ref().ok_or("shared memory unavailable")?;
    let manager = state
        .output_manager
        .as_ref()
        .ok_or("output geometry unavailable")?;
    let logical_outputs: Vec<_> = state
        .outputs
        .iter()
        .enumerate()
        .map(|(i, output)| manager.get_xdg_output(&output.output, &qh, i))
        .collect();
    let (shell, compositor, shm) = (shell.clone(), compositor.clone(), shm.clone());
    queue.roundtrip(&mut state)?;
    let origin_x = state.outputs.iter().map(|o| o.x).min().unwrap_or(0);
    let origin_y = state.outputs.iter().map(|o| o.y).min().unwrap_or(0);
    let (gx, gy) = (x + origin_x, y + origin_y);
    let output = state
        .outputs
        .iter()
        .find(|o| gx >= o.x && gy >= o.y && gx < o.x + o.w && gy < o.y + o.h)
        .ok_or("crossing output unavailable")?;
    let (left, top) = (gx - output.x, gy - output.y);
    let (w, h) = (w.min(output.w - left), h.min(output.h - top));
    let surface = compositor.create_surface(&qh, ());
    let input = compositor.create_region(&qh, ());
    surface.set_input_region(Some(&input));
    input.destroy();
    let layer = shell.get_layer_surface(
        &surface,
        Some(&output.output),
        Layer::Overlay,
        "mause-edge-flash".into(),
        &qh,
        (),
    );
    layer.set_anchor(Anchor::Top | Anchor::Left);
    layer.set_margin(top, 0, 0, left);
    layer.set_size(w as u32, h as u32);
    // Do not reserve desktop space, and ignore panel exclusive zones.
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    surface.commit();
    queue.roundtrip(&mut state)?;
    if !state.configured || state.closed {
        return Err("overlay not configured".into());
    }
    // Separate immutable buffers avoid writing pixels still owned by the compositor.
    let mut file = tempfile::tempfile()?;
    for color in super::FLASH_COLORS {
        let pixel = (0xFF00_0000 | color).to_ne_bytes();
        file.write_all(&pixel.repeat((w * h) as usize))?;
    }
    let bytes = w * h * 4;
    let pool = shm.create_pool(file.as_fd(), bytes * 2, &qh, ());
    let buffers: Vec<_> = (0..2)
        .map(|i| pool.create_buffer(i * bytes, w, h, w * 4, wl_shm::Format::Argb8888, &qh, ()))
        .collect();
    for step in 0..super::FLASH_STEPS {
        if state.closed {
            break;
        }
        surface.attach(Some(&buffers[step as usize % 2]), 0, 0);
        surface.damage(0, 0, w, h);
        surface.commit();
        queue.roundtrip(&mut state)?;
        std::thread::sleep(Duration::from_millis(super::FLASH_STEP_MS));
    }
    layer.destroy();
    surface.destroy();
    for buffer in buffers {
        buffer.destroy();
    }
    pool.destroy();
    for output in logical_outputs {
        output.destroy();
    }
    conn.flush()?;
    Ok(())
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_compositor" => {
                    state.compositor = Some(registry.bind(name, version.min(4), qh, ()))
                }
                "wl_shm" => state.shm = Some(registry.bind(name, 1, qh, ())),
                "zwlr_layer_shell_v1" => state.shell = Some(registry.bind(name, 1, qh, ())),
                "zxdg_output_manager_v1" => {
                    state.output_manager = Some(registry.bind(name, version.min(2), qh, ()))
                }
                "wl_output" => {
                    let output = registry.bind(name, version.min(2), qh, ());
                    state.outputs.push(Output {
                        output,
                        x: 0,
                        y: 0,
                        w: 0,
                        h: 0,
                    });
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<zxdg_output_v1::ZxdgOutputV1, usize> for State {
    fn event(
        state: &mut Self,
        _: &zxdg_output_v1::ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let output = &mut state.outputs[*index];
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                output.x = x;
                output.y = y;
            }
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                output.w = width;
                output.h = height;
            }
            _ => {}
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for State {
    fn event(
        state: &mut Self,
        layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, .. } => {
                layer.ack_configure(serial);
                state.configured = true;
            }
            zwlr_layer_surface_v1::Event::Closed => state.closed = true,
            _ => {}
        }
    }
}

delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore wl_buffer::WlBuffer);
delegate_noop!(State: ignore wl_surface::WlSurface);
delegate_noop!(State: ignore wl_region::WlRegion);
delegate_noop!(State: ignore wl_output::WlOutput);
delegate_noop!(State: ignore zxdg_output_manager_v1::ZxdgOutputManagerV1);
delegate_noop!(State: ignore zwlr_layer_shell_v1::ZwlrLayerShellV1);

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "desktop: flashes the four screen edges in a real Wayland layer-shell session"]
    fn real_wayland_compositor_accepts_all_four_edge_overlays() {
        assert!(super::super::wayland::active());
        let (w, h) = super::super::detect_screen().expect("screen geometry");
        for (x, y, vertical) in [
            (0, h / 2, true),
            (w - 1, h / 2, true),
            (w / 2, 0, false),
            (w / 2, h - 1, false),
        ] {
            let (x, y, w, h) = super::super::flash_rect(x, y, vertical, w, h);
            super::show(x, y, w, h).expect("native Wayland edge flash");
        }
    }
}
