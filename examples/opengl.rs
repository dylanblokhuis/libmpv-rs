// Copyright (C) 2021  anlumo
//
// This file is part of mpv-rs.
//
// This library is free software; you can redistribute it and/or
// modify it under the terms of the GNU Lesser General Public
// License as published by the Free Software Foundation; either
// version 2.1 of the License, or (at your option) any later version.
//
// This library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public
// License along with this library; if not, write to the Free Software
// Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA  02110-1301  USA

use glutin::{
    event::{Event, WindowEvent},
    event_loop::ControlFlow,
    window::Window,
    ContextWrapper, PossiblyCurrent,
};
use libmpv::{render::RenderContext, FileState, Mpv};

use std::{
    env,
    ffi::{c_char, c_void, CStr},
};

#[derive(Debug)]
enum MPVEvent {
    RenderUpdate,
    EventUpdate,
}

unsafe extern "C" fn get_proc_addr(ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    let rust_name = CStr::from_ptr(name).to_str().unwrap();
    let window: &ContextWrapper<PossiblyCurrent, Window> = std::mem::transmute(ctx);
    window.get_proc_address(rust_name) as *mut _
}

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;

fn main() {
    let path = env::args()
        .nth(1)
        .expect("Please provide a path to a video file");

    let (_, _, window, events_loop) = unsafe {
        let evloop = glutin::event_loop::EventLoop::<MPVEvent>::with_user_event();
        let window_builder = glutin::window::WindowBuilder::new()
            .with_title("true")
            .with_inner_size(glutin::dpi::LogicalSize::new(WIDTH, HEIGHT));
        let window = glutin::ContextBuilder::new()
            .with_vsync(true)
            .build_windowed(window_builder, &evloop)
            .expect("Failed to build glutin window")
            .make_current()
            .expect("Failed to make window current");
        let gl = glow::Context::from_loader_function(|l| window.get_proc_address(l) as *const _);
        (gl, "#version 140", window, evloop)
    };

    let mut mpv = Mpv::new().unwrap();

    let mut render_context =
        RenderContext::new(unsafe { mpv.ctx.as_mut() }, &window, get_proc_addr).unwrap();

    println!("Starting with {:?}", window.get_api());

    let event_proxy = events_loop.create_proxy();
    render_context.set_update_callback(move || {
        event_proxy.send_event(MPVEvent::RenderUpdate).unwrap();
    });
    let event_proxy = events_loop.create_proxy();
    mpv.event_context_mut().set_wakeup_callback(move || {
        event_proxy.send_event(MPVEvent::EventUpdate).unwrap();
    });
    mpv.playlist_load_files(&[(&path, FileState::AppendPlay, None)])
        .unwrap();

    events_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::LoopDestroyed => {
                *control_flow = ControlFlow::Exit;
            }
            Event::MainEventsCleared => window.window().request_redraw(),
            Event::RedrawRequested(_) => {
                render_context.render(WIDTH as i32, HEIGHT as i32).unwrap();
                window.swap_buffers().unwrap();
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(ue) => match ue {
                MPVEvent::RenderUpdate => {
                    render_context.update();
                    window.window().request_redraw();
                }
                MPVEvent::EventUpdate => loop {
                    match mpv.event_context_mut().wait_event(0.0) {
                        Some(Ok(libmpv::events::Event::EndFile(_))) => {
                            *control_flow = ControlFlow::Exit;
                            break;
                        }
                        Some(Ok(mpv_event)) => {
                            println!("MPV event: {:?}", mpv_event);
                        }
                        Some(Err(err)) => {
                            println!("MPV Error: {}", err);
                            *control_flow = ControlFlow::Exit;
                            break;
                        }
                        None => {
                            *control_flow = ControlFlow::Wait;
                            break;
                        }
                    }
                },
            },
            _ => {} /*Event::DeviceEvent { device_id, event } => todo!(),
                    Event::UserEvent(_) => todo!(),
                    Event::Suspended => todo!(),
                    Event::Resumed => todo!(),
                    Event::RedrawEventsCleared => todo!(),*/
        }
    });
}
