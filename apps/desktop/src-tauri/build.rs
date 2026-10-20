/// tauri-build's default manifest (Common-Controls v6) plus the Windows 8+
/// compatibility section: Windows supports `WS_EX_LAYERED` on a *child* window
/// only for a process that declares Windows 8 or later, and the taskbar widget
/// (ADR-0084) is a layered child of the taskbar.
const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}"/>
      <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
</assembly>
"#;

fn main() {
	// On MSVC the manifest is embedded by the linker rather than by
	// tauri-build's resource file, so that test binaries get it too: the tray
	// icon links comctl32 v6's `TaskDialogIndirect` through muda, and a binary
	// without the Common-Controls v6 dependency dies at load with
	// STATUS_ENTRYPOINT_NOT_FOUND; the embed test needs the layered child.
	let windows_msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
		&& std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
	if !windows_msvc {
		tauri_build::build();
		return;
	}
	let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
	tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
		.expect("tauri-build failed");
	let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
	let path = out.join("factorai.manifest");
	std::fs::write(&path, MANIFEST).expect("write the app manifest");
	println!("cargo:rerun-if-changed=build.rs");
	// `rustc-link-arg` also reaches the cdylib; a manifest in a DLL is inert, so harmless.
	println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
	// No UAC section: the linker would otherwise add one tauri-build's lacks.
	println!("cargo:rustc-link-arg=/MANIFESTUAC:NO");
	println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", path.display());
}
