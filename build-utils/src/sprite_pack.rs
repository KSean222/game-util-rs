use std::path::Path;
use texture_packer::{MultiTexturePacker, TexturePackerConfig, Frame, Rect};
use texture_packer::importer::ImageImporter;
use image::DynamicImage;
use std::collections::HashMap;
use regex::Regex;
use std::ops::Deref;
use std::collections::hash_map::Entry;
use texture_packer::exporter::ImageExporter;
use std::fs::File;
use std::io::{ BufWriter, Write };

pub fn gen_sprites(root: impl AsRef<Path>, target: impl AsRef<Path>, size: u32) {
    let mut packer = MultiTexturePacker::new_skyline(TexturePackerConfig {
        max_width: size,
        max_height: size,
        ..Default::default()
    });
    
    let mut entries = HashMap::new();
    
    let root = root.as_ref();
    println!("cargo:rerun-if-changed={}", root.display());
    process_dir(&mut entries, &mut packer, root, None);
    
    let target = target.as_ref();
    std::fs::create_dir_all(target).unwrap();
    for (i, page) in packer.get_pages().iter().enumerate() {
        let img = ImageExporter::export(page).unwrap();
        img.save(target.join(&format!("{}.png", i))).unwrap();
    }
    
    for (name, k) in &entries {
        match k {
            Kind::Array(v) => for (i, o) in v.iter().enumerate() {
                if o.is_none() {
                    panic!("index {} of sprite array {} is missing", i, name);
                }
            }
            Kind::Just(_) => {}
        }
    }
    
    let mut sprites = BufWriter::new(
        File::create(target.join("sprites.rs")).unwrap()
    );
    
    write!(sprites, "mod sprites {{\
        use game_util::Sprite;\
        use game_util::prelude::*;\
        use game_util::image;\
        use gl::types::*;\
        pub struct Sprites {{").unwrap();
    
    for (name, kind) in &entries {
        match kind {
            Kind::Just(_) => write!(sprites, "pub {}: Sprite,", name).unwrap(),
            Kind::Array(v) => write!(sprites, "pub {}: [Sprite; {}],", name, v.len()).unwrap()
        }
    }
    
    writeln!(sprites, "}}").unwrap();
    
    write!(sprites, "impl Sprites {{ pub fn load() -> (Self, GLuint) {{\
        let mut tex = 0;\
        unsafe {{\
            gl::GenTextures(1, &mut tex);\
            gl::BindTexture(gl::TEXTURE_2D_ARRAY, tex);\
            gl::TexImage3D(gl::TEXTURE_2D_ARRAY, 0, gl::RGBA8 as _, {}, {0}, {}, 0, gl::RGBA, gl::UNSIGNED_BYTE, 0 as *const _);\
            gl::TexParameteri(gl::TEXTURE_2D_ARRAY, gl::TEXTURE_MIN_FILTER, gl::LINEAR as _);", size, packer.get_pages().len()
    ).unwrap();
    
    for i in 0..packer.get_pages().len() {
        write!(sprites, "let img = image::load_from_memory_with_format(include_bytes!(\"{}.png\") as &[_], image::ImageFormat::Png).unwrap();", i).unwrap();
        write!(sprites, "let img = img.as_rgba8().unwrap(); gl::TexSubImage3D(\
                gl::TEXTURE_2D_ARRAY,\
                0,\
                0, 0, {},\
                img.width() as _, img.height() as _, 1,\
                gl::RGBA,\
                gl::UNSIGNED_BYTE,\
                img.as_ptr() as _\
        );", i).unwrap()
    }
    
    write!(sprites, "}} (Sprites {{").unwrap();
    
    for (name, kind) in &entries {
        write!(sprites, "{}: ", name).unwrap();
        match kind {
            Kind::Just(data) => write_sprite(&mut sprites, data, size),
            Kind::Array(v) => {
                write!(sprites, "[").unwrap();
                for data in v {
                    write_sprite(&mut sprites, data.as_ref().unwrap(), size);
                }
                write!(sprites, "],").unwrap();
            }
        }
        
        fn write_sprite(sprites: &mut impl Write, data: &Data, size: u32) {
            write!(
                sprites,
                "Sprite {{\
                    tex: rect({}.0 / {}.0, {}.0 / {1}.0, {}.0 / {1}.0, {}.0 / {1}.0),\
                    trimmed_size: size2({3}.0, {4}.0),\
                    real_size: size2({}.0, {}.0),\
                    layer: {}.0,\
                    rotated: {}\
                }},",
                data.tex.x, size, data.tex.y,
                data.tex.w, data.tex.h,
                data.real_size.0, data.real_size.1,
                data.layer,
                data.rotated
            ).unwrap();
        }
    }
    
    write!(sprites, "}}, tex)}}}}}}").unwrap();
}

fn process_dir(
    entries: &mut HashMap<String, Kind>,
    packer: &mut MultiTexturePacker<DynamicImage>,
    path: &Path,
    field_name: Option<String>
) {
    for entry in std::fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        println!("cargo:rerun-if-changed={}", entry.path().display());
        let t = entry.file_type().unwrap();
        let file_name = entry.file_name();
        let (name, array) = process_name(field_name.as_ref().map(Deref::deref), &file_name.to_string_lossy());
    
        if t.is_dir() {
            process_dir(entries, packer, &entry.path(), Some(name));
        } else if t.is_file() {
            let key = match array {
                Some(i) => format!("{}[{}]", name, i),
                None => name.clone()
            };
            packer.pack_own(key.clone(), ImageImporter::import_from_file(&entry.path()).unwrap()).unwrap();
            let mut frame = None;
            for (i, page) in packer.get_pages().iter().enumerate() {
                if let Some(f) = page.get_frame(&key) {
                    frame = Some((i, f));
                }
            }
            let frame = frame.unwrap();
        
            if let Some(i) = array {
                let v = entries.entry(name.clone()).or_insert(Kind::Array(vec![]));
                match v {
                    Kind::Array(v) => {
                        while v.len() <= i {
                            v.push(None);
                        }
                        if v[i].is_some() {
                            panic!("??? two of the same index?");
                        }
                        v[i] = Some(Data::from(frame));
                    }
                    Kind::Just(_) => panic!("mixing sprite and array of sprites at {}", name)
                }
            } else {
                match entries.entry(name.clone()) {
                    Entry::Occupied(_) => {
                        panic!("there's already a sprite called {}", name);
                    },
                    Entry::Vacant(e) => {
                        e.insert(Kind::Just(Data::from(frame)));
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
enum Kind {
    Just(Data),
    Array(Vec<Option<Data>>)
}

#[derive(Debug)]
struct Data {
    tex: Rect,
    real_size: (u32, u32),
    layer: usize,
    rotated: bool,
}

impl From<(usize, &Frame)> for Data {
    fn from((layer, frame): (usize, &Frame)) -> Self {
        Data {
            tex: frame.frame,
            real_size: (frame.source.w, frame.source.h),
            layer,
            rotated: frame.rotated
        }
    }
}

fn process_name(parent_name: Option<&str>, name: &str) -> (String, Option<usize>) {
    lazy_static::lazy_static! {
        static ref REGEX: Regex = Regex::new(r"^([_a-zA-Z][_\w]*)(?:.(\d+))?\.\w+$").unwrap();
    };
    
    match REGEX.captures(name) {
        Some(caps) => {
            let name = caps.get(1).unwrap().as_str();
            let name = match parent_name {
                Some(p) => format!("{}_{}", p, name),
                None => name.to_owned()
            };
            let index = caps.get(2).map(|m| m.as_str().parse().unwrap());
            (name, index)
        }
        None => panic!("invalid name: {}", name)
    }
}