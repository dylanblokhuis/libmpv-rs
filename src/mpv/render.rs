// Copyright (C) 2016  ParadoxSpiral
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

use crate::{mpv::mpv_err, Error, Result};
use libmpv_sys::{
    self, mpv_handle, mpv_opengl_init_params, mpv_render_context, mpv_render_context_free,
    mpv_render_context_render, mpv_render_context_set_update_callback, mpv_render_context_update,
    mpv_render_frame_info, mpv_render_param,
    mpv_render_param_type_MPV_RENDER_PARAM_ADVANCED_CONTROL,
    mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE,
    mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_INIT_PARAMS, MPV_RENDER_API_TYPE_OPENGL,
};
use std::convert::From;
use std::ffi::{c_void, CStr};
use std::mem::transmute;
use std::os::raw::c_int;
use std::ptr::{self, null_mut};

pub struct RenderContext {
    ctx: *mut mpv_render_context,
    render_params: Vec<mpv_render_param>,
    update_callback_cleanup: Option<Box<dyn FnOnce()>>,
}

/// For initializing the mpv OpenGL state via RenderParam::OpenGLInitParams
pub struct OpenGLInitParams<GLContext> {
    /// This retrieves OpenGL function pointers, and will use them in subsequent
    /// operation.
    /// Usually, you can simply call the GL context APIs from this callback (e.g.
    /// glXGetProcAddressARB or wglGetProcAddress), but some APIs do not always
    /// return pointers for all standard functions (even if present); in this
    /// case you have to compensate by looking up these functions yourself when
    /// libmpv wants to resolve them through this callback.
    /// libmpv will not normally attempt to resolve GL functions on its own, nor
    /// does it link to GL libraries directly.
    pub get_proc_address: fn(ctx: &GLContext, name: &str) -> *mut c_void,

    /// Value passed as ctx parameter to get_proc_address().
    pub ctx: GLContext,
}

/// For RenderParam::FBO
pub struct FBO {
    pub fbo: i32,
    pub width: i32,
    pub height: i32,
}

#[repr(u32)]
#[derive(Clone)]
pub enum RenderFrameInfoFlag {
    Present = libmpv_sys::mpv_render_frame_info_flag_MPV_RENDER_FRAME_INFO_PRESENT,
    Redraw = libmpv_sys::mpv_render_frame_info_flag_MPV_RENDER_FRAME_INFO_REDRAW,
    Repeat = libmpv_sys::mpv_render_frame_info_flag_MPV_RENDER_FRAME_INFO_REPEAT,
    BlockVSync = libmpv_sys::mpv_render_frame_info_flag_MPV_RENDER_FRAME_INFO_BLOCK_VSYNC,
}

impl From<u64> for RenderFrameInfoFlag {
    // mpv_render_frame_info_flag is u32, but mpv_render_frame_info.flags is u64 o\
    fn from(val: u64) -> Self {
        let val = val as u32;
        match val {
            libmpv_sys::mpv_render_frame_info_flag_MPV_RENDER_FRAME_INFO_PRESENT => {
                RenderFrameInfoFlag::Present
            }
            libmpv_sys::mpv_render_frame_info_flag_MPV_RENDER_FRAME_INFO_REDRAW => {
                RenderFrameInfoFlag::Redraw
            }
            libmpv_sys::mpv_render_frame_info_flag_MPV_RENDER_FRAME_INFO_REPEAT => {
                RenderFrameInfoFlag::Repeat
            }
            libmpv_sys::mpv_render_frame_info_flag_MPV_RENDER_FRAME_INFO_BLOCK_VSYNC => {
                RenderFrameInfoFlag::BlockVSync
            }
            _ => panic!("Tried converting invalid value to RenderFrameInfoFlag"),
        }
    }
}

#[derive(Clone)]
pub struct RenderFrameInfo {
    pub flags: RenderFrameInfoFlag,
    pub target_time: i64,
}

pub enum RenderParamApiType {
    OpenGl,
}

pub enum RenderParam<GLContext> {
    Invalid,
    ApiType(RenderParamApiType),
    InitParams(OpenGLInitParams<GLContext>),
    FBO(FBO),
    FlipY(bool),
    Depth(i32),
    ICCProfile(Vec<u8>),
    AmbientLight(i32),
    X11Display(*const c_void),
    WaylandDisplay(*const c_void),
    AdvancedControl(bool),
    NextFrameInfo(RenderFrameInfo),
    BlockForTargetTime(bool),
    SkipRendering(bool),
}

impl<C> From<&RenderParam<C>> for u32 {
    fn from(val: &RenderParam<C>) -> Self {
        match val {
            RenderParam::Invalid => 0,
            RenderParam::ApiType(_) => 1,
            RenderParam::InitParams(_) => 2,
            RenderParam::FBO(_) => 3,
            RenderParam::FlipY(_) => 4,
            RenderParam::Depth(_) => 5,
            RenderParam::ICCProfile(_) => 6,
            RenderParam::AmbientLight(_) => 7,
            RenderParam::X11Display(_) => 8,
            RenderParam::WaylandDisplay(_) => 9,
            RenderParam::AdvancedControl(_) => 10,
            RenderParam::NextFrameInfo(_) => 11,
            RenderParam::BlockForTargetTime(_) => 12,
            RenderParam::SkipRendering(_) => 13,
        }
    }
}

unsafe extern "C" fn gpa_wrapper<GLContext>(ctx: *mut c_void, name: *const i8) -> *mut c_void {
    if ctx.is_null() {
        panic!("ctx for get_proc_address wrapper is NULL");
    }

    let params: *mut OpenGLInitParams<GLContext> = ctx as _;
    let params = &*params;

    let gl_fn = CStr::from_ptr(name)
        .to_str()
        .expect("Could not convert function name to str");
    println!("gl: {gl_fn}");

    (params.get_proc_address)(&params.ctx, gl_fn)
}

unsafe extern "C" fn ru_wrapper<F: Fn() + Send + 'static>(ctx: *mut c_void) {
    if ctx.is_null() {
        panic!("ctx for render_update wrapper is NULL");
    }

    (*(ctx as *mut F))();
}

impl<C> From<OpenGLInitParams<C>> for mpv_opengl_init_params {
    fn from(val: OpenGLInitParams<C>) -> Self {
        Self {
            get_proc_address: Some(gpa_wrapper::<OpenGLInitParams<C>>),
            get_proc_address_ctx: Box::into_raw(Box::new(val)) as *mut c_void,
        }
    }
}

impl<C> From<RenderParam<C>> for mpv_render_param {
    fn from(val: RenderParam<C>) -> Self {
        let type_ = u32::from(&val);
        let data = match val {
            RenderParam::Invalid => ptr::null_mut(),
            RenderParam::ApiType(api_type) => match api_type {
                RenderParamApiType::OpenGl => {
                    libmpv_sys::MPV_RENDER_API_TYPE_OPENGL.as_ptr() as *mut c_void
                }
            },
            RenderParam::InitParams(params) => {
                Box::into_raw(Box::new(mpv_opengl_init_params::from(params))) as *mut c_void
            }
            RenderParam::FBO(fbo) => Box::into_raw(Box::new(fbo)) as *mut c_void,
            RenderParam::FlipY(flip) => Box::into_raw(Box::new(flip as c_int)) as *mut c_void,
            RenderParam::Depth(depth) => Box::into_raw(Box::new(depth)) as *mut c_void,
            RenderParam::ICCProfile(bytes) => {
                Box::into_raw(bytes.into_boxed_slice()) as *mut c_void
            }
            RenderParam::AmbientLight(lux) => Box::into_raw(Box::new(lux)) as *mut c_void,
            RenderParam::X11Display(ptr) => ptr as *mut _,
            RenderParam::WaylandDisplay(ptr) => ptr as *mut _,
            RenderParam::AdvancedControl(adv_ctrl) => {
                Box::into_raw(Box::new(adv_ctrl as c_int)) as *mut c_void
            }
            RenderParam::NextFrameInfo(frame_info) => {
                Box::into_raw(Box::new(frame_info)) as *mut c_void
            }
            RenderParam::BlockForTargetTime(block) => {
                Box::into_raw(Box::new(block as c_int)) as *mut c_void
            }
            RenderParam::SkipRendering(skip_rendering) => {
                Box::into_raw(Box::new(skip_rendering as c_int)) as *mut c_void
            }
        };
        Self { type_, data }
    }
}

impl RenderContext {
    pub fn new<C>(
        mpv: &mut mpv_handle,
        ctx: C,
        get_proc_fn: unsafe extern "C" fn(*mut std::ffi::c_void, *const i8) -> *mut c_void,
    ) -> Result<Self> {
        // let params: Vec<_> = params.into_iter().collect();
        // let mut raw_params: Vec<mpv_render_param> = Vec::new();
        // raw_params.reserve(params.len() + 1);
        // let mut raw_ptrs: HashMap<*const c_void, DeleterFn> = HashMap::new();

        // for p in params {
        //     // The render params are type-erased after they are passed to mpv. This is where we last
        //     // know their real types, so we keep a deleter here.
        //     let deleter: Option<DeleterFn> = match p {
        //         RenderParam::InitParams(_) => Some(free_init_params::<C>),
        //         RenderParam::FBO(_) => Some(free_void_data::<FBO>),
        //         RenderParam::FlipY(_) => Some(free_void_data::<i32>),
        //         RenderParam::Depth(_) => Some(free_void_data::<i32>),
        //         RenderParam::ICCProfile(_) => Some(free_void_data::<Box<[u8]>>),
        //         RenderParam::AmbientLight(_) => Some(free_void_data::<i32>),
        //         RenderParam::NextFrameInfo(_) => Some(free_void_data::<RenderFrameInfo>),
        //         _ => None,
        //     };

        //     let raw_param: mpv_render_param = p.into();
        //     if let Some(deleter) = deleter {
        //         raw_ptrs.insert(raw_param.data, deleter);
        //     }

        //     raw_params.push(raw_param);
        // }
        // // the raw array must end with type = 0
        // raw_params.push(mpv_render_param {
        //     type_: 0,
        //     data: ptr::null_mut(),
        // });

        let mpv_ogl_init_param = unsafe {
            mpv_opengl_init_params {
                get_proc_address: Some(get_proc_fn),
                get_proc_address_ctx: transmute(&ctx),
            }
        };

        let mut mpv_render_params = unsafe {
            let vv = vec![
                mpv_render_param {
                    type_: mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE,
                    data: transmute(MPV_RENDER_API_TYPE_OPENGL),
                },
                mpv_render_param {
                    type_: mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                    data: transmute(&mpv_ogl_init_param),
                },
                mpv_render_param {
                    type_: mpv_render_param_type_MPV_RENDER_PARAM_ADVANCED_CONTROL,
                    data: transmute(&mut 1),
                },
                mpv_render_param {
                    // end??
                    type_: 0,
                    data: null_mut(),
                },
            ];
            vv
        };

        unsafe {
            let mut mpv_gl: *mut mpv_render_context = null_mut();
            let err = libmpv_sys::mpv_render_context_create(
                &mut mpv_gl,
                mpv,
                mpv_render_params.as_mut_ptr(),
            );

            mpv_err(
                Self {
                    ctx: mpv_gl,
                    render_params: mpv_render_params,
                    update_callback_cleanup: None,
                },
                err,
            )
        }
    }

    pub fn set_parameter<C>(&self, param: RenderParam<C>) -> Result<()> {
        unsafe {
            mpv_err(
                (),
                libmpv_sys::mpv_render_context_set_parameter(
                    self.ctx,
                    mpv_render_param::from(param),
                ),
            )
        }
    }

    pub fn get_info<C>(&self, param: RenderParam<C>) -> Result<RenderParam<C>> {
        let is_next_frame_info = matches!(param, RenderParam::NextFrameInfo(_));
        let raw_param = mpv_render_param::from(param);
        let res = unsafe { libmpv_sys::mpv_render_context_get_info(self.ctx, raw_param) };
        if res == 0 {
            if !is_next_frame_info {
                panic!("I don't know how to handle this info type.");
            }
            let raw_frame_info = raw_param.data as *mut mpv_render_frame_info;
            unsafe {
                let raw_frame_info = *raw_frame_info;
                return Ok(RenderParam::NextFrameInfo(RenderFrameInfo {
                    flags: raw_frame_info.flags.into(),
                    target_time: raw_frame_info.target_time,
                }));
            }
        }
        Err(Error::Raw(res))
    }

    /// Render video.
    ///
    /// Typically renders the video to a target surface provided via `fbo`
    /// (the details depend on the backend in use). Options like "panscan" are
    /// applied to determine which part of the video should be visible and how the
    /// video should be scaled. You can change these options at runtime by using the
    /// mpv property API.
    ///
    /// The renderer will reconfigure itself every time the target surface
    /// configuration (such as size) is changed.
    ///
    /// This function implicitly pulls a video frame from the internal queue and
    /// renders it. If no new frame is available, the previous frame is redrawn.
    /// The update callback set with [set_update_callback](Self::set_update_callback)
    /// notifies you when a new frame was added. The details potentially depend on
    /// the backends and the provided parameters.
    ///
    /// Generally, libmpv will invoke your update callback some time before the video
    /// frame should be shown, and then lets this function block until the supposed
    /// display time. This will limit your rendering to video FPS. You can prevent
    /// this by setting the "video-timing-offset" global option to 0. (This applies
    /// only to "audio" video sync mode.)
    ///
    /// # Arguments
    ///
    /// * `fbo` - A framebuffer object to render to. In OpenGL, 0 is the current backbuffer
    /// * `width` - The width of the framebuffer in pixels. This is used for scaling the
    ///             video properly.
    /// * `height` - The height of the framebuffer in pixels. This is used for scaling the
    ///              video properly.
    /// * `flip` - Whether to draw the image upside down. This is needed for OpenGL because
    ///            it uses a coordinate system with positive Y up, but videos use positive
    ///            Y down.
    pub fn render(&mut self, width: i32, height: i32) -> Result<()> {
        self.render_params = unsafe {
            vec![
                mpv_render_param {
                    type_: libmpv_sys::mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_FBO,
                    data: transmute(&mut libmpv_sys::mpv_opengl_fbo {
                        fbo: 0,
                        w: width,
                        h: height,
                        internal_format: 0,
                    }),
                },
                // Why does MPV render upside down by default ):
                mpv_render_param {
                    type_: libmpv_sys::mpv_render_param_type_MPV_RENDER_PARAM_FLIP_Y,
                    data: transmute(&mut 1),
                },
                mpv_render_param {
                    type_: mpv_render_param_type_MPV_RENDER_PARAM_ADVANCED_CONTROL,
                    data: transmute(&mut 1),
                },
                mpv_render_param {
                    // end??
                    type_: 0,
                    data: null_mut(),
                },
            ]
        };

        unsafe {
            mpv_err(
                (),
                mpv_render_context_render(self.ctx, self.render_params.as_mut_ptr()),
            )
        }
    }

    /// Set the callback that notifies you when a new video frame is available, or if the video display
    /// configuration somehow changed and requires a redraw. Similar to [EventContext::set_wakeup_callback](crate::events::EventContext::set_wakeup_callback), you
    /// must not call any mpv API from the callback, and all the other listed restrictions apply (such
    /// as not exiting the callback by throwing exceptions).
    ///
    /// This can be called from any thread, except from an update callback. In case of the OpenGL backend,
    /// no OpenGL state or API is accessed.
    ///
    /// Calling this will raise an update callback immediately.
    pub fn set_update_callback<F: Fn() + Send + 'static>(&mut self, callback: F) {
        if let Some(update_callback_cleanup) = self.update_callback_cleanup.take() {
            update_callback_cleanup();
        }
        let raw_callback = Box::into_raw(Box::new(callback));
        self.update_callback_cleanup = Some(Box::new(move || unsafe {
            Box::from_raw(raw_callback);
        }) as Box<dyn FnOnce()>);
        unsafe {
            mpv_render_context_set_update_callback(
                self.ctx,
                Some(ru_wrapper::<F>),
                raw_callback as *mut c_void,
            );
        }
    }

    pub fn update(&self) {
        unsafe {
            mpv_render_context_update(self.ctx);
        }
    }
}

impl Drop for RenderContext {
    fn drop(&mut self) {
        if let Some(update_callback_cleanup) = self.update_callback_cleanup.take() {
            update_callback_cleanup();
        }
        unsafe {
            mpv_render_context_free(self.ctx);
        }
    }
}
