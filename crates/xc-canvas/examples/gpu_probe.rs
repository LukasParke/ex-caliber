fn main() {
    env_logger::init();
    unsafe {
        match blade_graphics::Context::init(blade_graphics::ContextDesc {
            presentation: true,
            ..Default::default()
        }) {
            Ok(ctx) => println!("GPU context OK: {:?}", ctx.capabilities()),
            Err(e) => {
                eprintln!("GPU context FAILED: {e:#?}");
                std::process::exit(1);
            }
        }
    }
}
