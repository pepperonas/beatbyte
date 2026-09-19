//! Embeds platform resources in the BeatByte desktop executable.

fn main() {
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../packaging/icons/windows/BeatByte.ico");
        resource
            .compile()
            .expect("failed to embed the BeatByte Windows icon");
    }
}
