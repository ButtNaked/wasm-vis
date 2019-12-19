
mod utils;

#[macro_use]
extern crate serde_derive;

use wasm_bindgen::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{WebGlProgram, WebGlRenderingContext, WebGlShader, WebGlUniformLocation, console};

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
// allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen]
extern {
    fn alert(s: &str);
}

#[wasm_bindgen]
pub fn greet() {
    alert("Hello, wasm-vis!");
}

fn window() -> web_sys::Window {
    web_sys::window().expect("no global `window` exists")
}

fn request_animation_frame(f: &Closure<dyn FnMut()>) {
    window()
        .request_animation_frame(f.as_ref().unchecked_ref())
        .expect("should register `requestAnimationFrame` OK");
}

fn document() -> web_sys::Document {
    window()
        .document()
        .expect("should have a document on window")
}

fn body() -> web_sys::HtmlElement {
    document().body().expect("document should have a body")
}


const BUF_MAX_SZ : usize = 4000*2;
static mut BUF : [f32; BUF_MAX_SZ] = [0f32;BUF_MAX_SZ];
static mut BUF_SZ : usize = 0;
static mut BUF_RD : usize = 0;
static mut BUF_I : usize  = 0;

fn add_point(amp : f32) {
    unsafe {
        BUF[BUF_RD + 0] = BUF_I as f32;
        BUF[BUF_RD + 1] = amp;
        BUF_RD += 2;
        BUF_RD %= BUF_SZ;
        BUF_I += 1;
        BUF_I %= BUF_SZ / 2;
    }
}

fn move_plot(offset : isize) {
    unsafe {
        let mut j = 0;
        for i in (0 .. BUF_SZ).step_by(2) {
            BUF[i]   = j as f32;
            BUF[i+1] = (i as f32 / 10. + (offset as f32)/2.).sin() * 50.0;
            j += 2;
        }
    }
}

#[wasm_bindgen(start)]
pub fn run() -> Result<(), JsValue> {
    // Set panic output to js console
    console_error_panic_hook::set_once();
    // Setup logger
    wasm_logger::init(wasm_logger::Config::default());
    
    let test = JsValue::from_str("MY js value");
    log::info!("Some info {:?}", &test);
    log::error!("Error message");

    let document = web_sys::window().unwrap().document().unwrap();
    let canvas = document.get_element_by_id("canvas").unwrap();
    let canvas: web_sys::HtmlCanvasElement = canvas.dyn_into::<web_sys::HtmlCanvasElement>()?;

    let height = canvas.offset_height() as u32;
    let width = canvas.offset_width() as u32;
    canvas.set_height(height);
    canvas.set_width(width);
    assert!(width <= BUF_MAX_SZ as u32 / 2);

    // Set buffer actual size
    unsafe { BUF_SZ = width as usize; }

    // Context settings
    #[derive(Serialize)]
    struct CxtCfg {
        antialias : bool,
        depth     : bool,
    };

    let cxt_cfg = CxtCfg { antialias : false, depth : false };
    let cxt_cfg = JsValue::from_serde(&cxt_cfg).unwrap();

    let context = canvas
        //.get_context("webgl")?
        .get_context_with_context_options("webgl", &cxt_cfg)?
        .unwrap()
        .dyn_into::<WebGlRenderingContext>()?;
        

    // Shaders
    let vert_shader = compile_shader(
        &context,
        WebGlRenderingContext::VERTEX_SHADER,
        r#"
        attribute vec2 a_position;
        uniform vec2 u_resolution;
        uniform vec2 u_shift;
        uniform vec2 u_xmod;

        // all shaders have a main function
        void main() 
        {
          vec2 Pos = a_position + u_shift; 
          
          if(u_xmod.x != 0.0 )
          {
            Pos.x = mod(Pos.x,u_xmod.x);
          };

          vec2 zeroToOne = Pos / u_resolution; // преобразуем положение в пикселях к диапазону от 0.0 до 1.0
       
          // преобразуем из 0->1 в 0->2
          vec2 zeroToTwo = zeroToOne * 2.0;
          // преобразуем из 0->2 в -1->+1 (пространство отсечения)
          vec2 clipSpace = zeroToTwo - 1.0;
          vec2 clipSpaceN = clipSpace * vec2(1, -1); // переворачиваем систему коооординат (0,0) в левом верхнем углу
     
          gl_Position = vec4(clipSpaceN, 0, 1);  
        }
    "#,
    )?;
    let frag_shader = compile_shader(
        &context,
        WebGlRenderingContext::FRAGMENT_SHADER,
        r#"
        void main() {
            gl_FragColor = vec4(0.0, 0.0, 0.0, 1.0);
        }
    "#,
    )?;
    let program = link_program(&context, &vert_shader, &frag_shader)?;
    context.use_program(Some(&program));

    //ATR
    //
    let position_attribute_location = context.get_attrib_location(&program, "a_position");
    let resolution_uniform_location = context.get_uniform_location(&program, "u_resolution");
    //context.get_uniform_location(&program, "u_color");
    let shift_location = context.get_uniform_location(&program, "u_shift");
    let xmod_location = context.get_uniform_location(&program, "u_xmod");

    let position_buffer = context.create_buffer().ok_or("failed to create buffer")?;
    context.bind_buffer(WebGlRenderingContext::ARRAY_BUFFER, Some(&position_buffer));

    // Note that `Float32Array::view` is somewhat dangerous (hence the
    // `unsafe`!). This is creating a raw view into our module's
    // `WebAssembly.Memory` buffer, but if we allocate more pages for ourself
    // (aka do a memory allocation in Rust) it'll cause the buffer to change,
    // causing the `Float32Array` to be invalid.
    //
    // As a result, after `Float32Array::view` we have to be very careful not to
    // do any memory allocations before it's dropped.
    
    context.enable_vertex_attrib_array(position_attribute_location as u32);
    context.vertex_attrib_pointer_with_i32(0, 2, WebGlRenderingContext::FLOAT, false, 0, 0);
    context.uniform2f(resolution_uniform_location.as_ref(), width as f32, height as f32);
    context.viewport(0,0, width as i32, height as i32);

    let f = Rc::new(RefCell::new(None));
    let g = f.clone();
    let mut cnt = 0;

    *g.borrow_mut() = Some(Closure::wrap(Box::new(move || {

        move_plot(cnt);
        cnt += 1;

        context.clear_color(1.0, 1.0, 1.0, 1.0);
        context.clear(WebGlRenderingContext::COLOR_BUFFER_BIT);

        unsafe {
            let vert_array = js_sys::Float32Array::view(&BUF[.. BUF_SZ]);

            context.buffer_data_with_array_buffer_view(
                WebGlRenderingContext::ARRAY_BUFFER,
                &vert_array,
                WebGlRenderingContext::DYNAMIC_DRAW,
            );
        }

        draw_plot(&context, &xmod_location, &shift_location, 100.);
        draw_plot(&context, &xmod_location, &shift_location, 200.);
        draw_plot(&context, &xmod_location, &shift_location, 300.);
        draw_plot(&context, &xmod_location, &shift_location, 400.);
        draw_plot(&context, &xmod_location, &shift_location, 500.);
        draw_plot(&context, &xmod_location, &shift_location, 600.);
        draw_plot(&context, &xmod_location, &shift_location, 700.);
        draw_plot(&context, &xmod_location, &shift_location, 800.);
        draw_plot(&context, &xmod_location, &shift_location, 900.);

        //context.finish();

        request_animation_frame(f.borrow().as_ref().unwrap());
    }) as Box<dyn FnMut()>));

    request_animation_frame(g.borrow().as_ref().unwrap());

    Ok(())
}

pub fn draw_plot(
    context : &WebGlRenderingContext, 
    xmod_location : &Option<WebGlUniformLocation>, 
    shift_location : &Option<WebGlUniformLocation>,
    shift_v : f32,
    ) {
    context.uniform2f(xmod_location.as_ref(), 0f32, 0f32);
    context.uniform2f(shift_location.as_ref(), 0f32, shift_v);
    context.draw_arrays(
        WebGlRenderingContext::LINE_STRIP,
        0,
        unsafe{BUF_SZ as i32 / 2},
    );
}

pub fn compile_shader(
    context: &WebGlRenderingContext,
    shader_type: u32,
    source: &str,
) -> Result<WebGlShader, String> {
    let shader = context
        .create_shader(shader_type)
        .ok_or_else(|| String::from("Unable to create shader object"))?;
    context.shader_source(&shader, source);
    context.compile_shader(&shader);

    if context
        .get_shader_parameter(&shader, WebGlRenderingContext::COMPILE_STATUS)
        .as_bool()
        .unwrap_or(false)
    {
        Ok(shader)
    } else {
        Err(context
            .get_shader_info_log(&shader)
            .unwrap_or_else(|| String::from("Unknown error creating shader")))
    }
}

pub fn link_program(
    context: &WebGlRenderingContext,
    vert_shader: &WebGlShader,
    frag_shader: &WebGlShader,
) -> Result<WebGlProgram, String> {
    let program = context
        .create_program()
        .ok_or_else(|| String::from("Unable to create shader object"))?;

    context.attach_shader(&program, vert_shader);
    context.attach_shader(&program, frag_shader);
    context.link_program(&program);

    if context
        .get_program_parameter(&program, WebGlRenderingContext::LINK_STATUS)
        .as_bool()
        .unwrap_or(false)
    {
        Ok(program)
    } else {
        Err(context
            .get_program_info_log(&program)
            .unwrap_or_else(|| String::from("Unknown error creating program object")))
    }
}