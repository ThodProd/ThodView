fn main() {
    println!("cargo:rerun-if-changed=assets/thodview.ico");
    println!("cargo:rerun-if-changed=assets/thodview.rc");
    embed_resource::compile_for("assets/thodview.rc", ["ThodView"], embed_resource::NONE)
        .manifest_required()
        .expect("Windows application resources");
}
