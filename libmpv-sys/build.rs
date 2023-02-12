// Copyright (C) 2016  ParadoxSpiral
//
// This file is part of mpv-sys.
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

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

#[cfg(not(feature = "use-bindgen"))]
fn main() {
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    let crate_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    std::fs::copy(
        crate_path.join("pregenerated_bindings.rs"),
        out_path.join("bindings.rs"),
    )
    .expect("Couldn't find pregenerated bindings!");

    println!("cargo:rerun-if-changed=include/client.h");
    println!("cargo:rerun-if-changed=include/render.h");
    println!("cargo:rerun-if-changed=include/render_gl.h");
    println!("cargo:rerun-if-changed=include/stream_cb.h");

    #[cfg(target_env = "msvc")]
    {
        download_and_compile_lib();
        println!("cargo:rustc-link-lib=static=mpv");
    }
    

    #[cfg(target_os = "linux")]
    {
        use_mpv_build();
        println!("cargo:rustc-link-lib=mpv");
    }
    

    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-lib=mpv");
}

#[cfg(target_os = "linux")]
fn use_mpv_build() {
    if pkg_config::Config::new()
        .atleast_version("0.35.0")
        .probe("libmpv")
        .is_ok()
    {
        println!("cargo:warning=Using system libmpv");
        return;
    }

    let install_dir = env::var("OUT_DIR").unwrap() + "/installed";
    let lib_install_dir = Path::new(&install_dir).join("lib");
    fs::create_dir_all(&lib_install_dir).unwrap();

    let archive_path = lib_install_dir.join("mpvbuild.zip");
    if fs::File::open(archive_path.clone()).is_err() {
        let mpv_zip = "https://github.com/mpv-player/mpv-build/archive/refs/heads/master.zip";
        let res = reqwest::blocking::get(mpv_zip).unwrap();
        let bytes = res.bytes().unwrap();

        // write file
        let mut file = std::fs::File::create(archive_path.clone()).unwrap();
        file.write_all(&bytes).unwrap();
    }

    let extracted_files_path = lib_install_dir.join("files");
    if fs::File::open(lib_install_dir.join("files/rebuild")).is_err() {
        let archive_file = fs::File::open(archive_path.clone()).unwrap();
        let mut archive = zip::ZipArchive::new(archive_file).unwrap();

        // panic!("{}", archive_path.display());
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).unwrap();
            let outpath = match file.enclosed_name() {
                Some(path) => extracted_files_path.join(path),
                None => continue,
            };

            {
                let comment = file.comment();
                if !comment.is_empty() {
                    println!("File {i} comment: {comment}");
                }
            }

            if (*file.name()).ends_with('/') {
                println!("File {} extracted to \"{}\"", i, outpath.display());
                fs::create_dir_all(&outpath).unwrap();
            } else {
                println!(
                    "File {} extracted to \"{}\" ({} bytes)",
                    i,
                    outpath.display(),
                    file.size()
                );
                if let Some(p) = outpath.parent() {
                    if !p.exists() {
                        fs::create_dir_all(p).unwrap();
                    }
                }
                let mut outfile = fs::File::create(&outpath).unwrap();
                io::copy(&mut file, &mut outfile).unwrap();
            }

            // Get and Set permissions
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                if let Some(mode) = file.unix_mode() {
                    fs::set_permissions(&outpath, fs::Permissions::from_mode(mode)).unwrap();
                }
            }
        }
    }

    if fs::File::open(extracted_files_path.join("mpv-build-master/mpv/build/mpv")).is_err() {
        // create mpv_options file
        {
            let mut mpv_options =
                fs::File::create(extracted_files_path.join("mpv-build-master/mpv_options"))
                    .unwrap();
            mpv_options.write_all("-Dlibmpv=true".as_bytes()).unwrap();
        }

        // build mpv binary
        let output = Command::new("sh")
            .current_dir(extracted_files_path.join("mpv-build-master"))
            .env("CFLAGS", "-fPIC")
            .arg("./rebuild")
            .arg("-j4")
            .output()
            .expect("failed to execute process");

        let output = String::from_utf8(output.stdout).unwrap();
        if !output.contains("Linking target mpv") {
            panic!("{}\nFailed to build mpv using mpv-build", output);
        }
    }

    println!(
        "cargo:rustc-link-search=native={}",
        extracted_files_path
            .join("mpv-build-master/mpv/build")
            .display()
    );
}

#[cfg(target_env = "msvc")]
fn download_and_compile_lib() {
    use std::{fs, io::Write, path::Path, process::Command};

    let install_dir = env::var("OUT_DIR").unwrap() + "/installed";
    let lib_install_dir = Path::new(&install_dir).join("lib");
    fs::create_dir_all(&lib_install_dir).unwrap();

    let archive_path = lib_install_dir.join("mpv.7z");
    if fs::File::open(archive_path.clone()).is_err() {
        let mpv_zip = "https://kumisystems.dl.sourceforge.net/project/mpv-player-windows/libmpv/mpv-dev-x86_64-v3-20221113-git-2f74734.7z";
        let res = reqwest::blocking::get(mpv_zip).unwrap();
        let bytes = res.bytes().unwrap();

        // write file
        let mut file = std::fs::File::create(archive_path.clone()).unwrap();
        file.write_all(&bytes).unwrap();
    }

    let extracted_files_path = lib_install_dir.join("files");
    if fs::File::open(extracted_files_path.join("mpv.lib")).is_err() {
        sevenz_rust::decompress_file(archive_path, extracted_files_path.clone()).expect("complete");

        // add EXPORTS to mpv.def, otherwise the mpv.lib will be empty
        let mut mpv_def = fs::read_to_string(extracted_files_path.join("mpv.def")).unwrap();
        mpv_def = format!("EXPORTS\n{}", mpv_def);
        fs::write(extracted_files_path.join("mpv.def"), mpv_def).unwrap();

        let cmd_output = Command::new("lib.exe")
            .current_dir(extracted_files_path.clone())
            .arg("/def:mpv.def")
            .arg("/name:mpv-2.dll")
            .arg("/out:mpv.lib")
            .arg("/MACHINE:X64")
            .output()
            .expect("Failed to run lib.exe, do you have Visual Studio Build Tools installed?");

        let output = String::from_utf8(cmd_output.stdout).unwrap();
        if !output.contains("Creating library mpv.lib and object mpv.exp") {
            panic!("lib.exe failed: {}", output);
        }
    }

    println!(
        "cargo:rustc-link-search=native={}",
        extracted_files_path.display()
    );
}

#[cfg(feature = "use-bindgen")]
fn main() {
    let bindings = bindgen::Builder::default()
        .header("include/client.h")
        .header("include/render.h")
        .header("include/render_gl.h")
        .header("include/stream_cb.h")
        .impl_debug(true)
        .opaque_type("mpv_handle")
        .opaque_type("mpv_render_context")
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    bindings
        .write_to_file("pregenerated_bindings.rs")
        .expect("Couldn't write bindings!");

    println!("cargo:rerun-if-changed=include/client.h");
    println!("cargo:rerun-if-changed=include/render.h");
    println!("cargo:rerun-if-changed=include/render_gl.h");
    println!("cargo:rerun-if-changed=include/stream_cb.h");
    println!("cargo:rustc-link-lib=static=mpv");
}
