//! 3MF export (3MF core spec). The shell and the mount are separate closed
//! meshes combined as components of one object, so Bambu Studio / Orca /
//! PrusaSlicer load a single object with several parts and union them.

use crate::geometry::{build_insert, build_mount, build_shell_parts, export_res, split_cuts};
use crate::mesh::Mesh;
use crate::project::Project;
use glam::Vec3;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub struct ExportInfo {
    pub triangles: usize,
    pub volume_mm3: f64,
    /// Second file with the glue-in mount insert (vase mode).
    pub insert_path: Option<std::path::PathBuf>,
    /// Upper shade parts of a split shade (part 2, 3, ...); part 1 is the main file.
    pub part_paths: Vec<std::path::PathBuf>,
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
 <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
 <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>
</Types>"#;

const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>"#;

pub fn write_3mf(path: &Path, title: &str, parts: &[(String, Mesh)], offset: Vec3) -> std::io::Result<()> {
    let file = File::create(path)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("[Content_Types].xml", opts)?;
    zip.write_all(CONTENT_TYPES.as_bytes())?;
    zip.start_file("_rels/.rels", opts)?;
    zip.write_all(RELS.as_bytes())?;
    zip.start_file("3D/3dmodel.model", opts)?;
    {
        let mut w = BufWriter::with_capacity(1 << 20, &mut zip);
        writeln!(w, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
        writeln!(
            w,
            r#"<model unit="millimeter" xml:lang="en-US" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">"#
        )?;
        writeln!(w, r#" <metadata name="Title">{}</metadata>"#, xml_escape(title))?;
        writeln!(w, r#" <metadata name="Application">Lampdesigner</metadata>"#)?;
        writeln!(w, " <resources>")?;
        for (k, (name, m)) in parts.iter().enumerate() {
            writeln!(w, r#"  <object id="{}" type="model" name="{}">"#, k + 1, xml_escape(name))?;
            writeln!(w, "   <mesh>\n    <vertices>")?;
            for p in &m.pos {
                writeln!(w, r#"     <vertex x="{:.4}" y="{:.4}" z="{:.4}"/>"#, p.x, p.y, p.z)?;
            }
            writeln!(w, "    </vertices>\n    <triangles>")?;
            for t in &m.tris {
                writeln!(w, r#"     <triangle v1="{}" v2="{}" v3="{}"/>"#, t[0], t[1], t[2])?;
            }
            writeln!(w, "    </triangles>\n   </mesh>\n  </object>")?;
        }
        let assembly = parts.len() + 1;
        writeln!(w, r#"  <object id="{}" type="model" name="{}">"#, assembly, xml_escape(title))?;
        writeln!(w, "   <components>")?;
        for k in 0..parts.len() {
            writeln!(w, r#"    <component objectid="{}"/>"#, k + 1)?;
        }
        writeln!(w, "   </components>\n  </object>\n </resources>")?;
        writeln!(
            w,
            r#" <build>
  <item objectid="{}" transform="1 0 0 0 1 0 0 0 1 {:.4} {:.4} {:.4}"/>
 </build>
</model>"#,
            assembly, offset.x, offset.y, offset.z
        )?;
        w.flush()?;
    }
    zip.finish()?;
    Ok(())
}

pub fn export_project(p: &Project, path: &Path, fine: bool) -> Result<ExportInfo, String> {
    let d = &p.design;
    let (nt, nz) = export_res(d, fine);
    let mut shells = build_shell_parts(d, nt, nz);
    for s in &shells {
        s.check_closed().map_err(|e| format!("shell mesh not watertight: {e}"))?;
    }
    let n_parts = shells.len();
    let upper = shells.split_off(1);
    let shell = shells.pop().expect("one shell part");
    let name = if n_parts > 1 { format!("Shade part 1/{n_parts}") } else { "Shade".to_string() };
    let mut parts = vec![(name, shell)];
    let mount = build_mount(d, 1440);
    let windowed = mount.len() > 2;
    for (k, m) in mount.into_iter().enumerate() {
        m.check_closed().map_err(|e| format!("mount mesh not watertight: {e}"))?;
        let name = match k {
            0 => "Mount spider".to_string(),
            1 if !windowed => "Socket sleeve".to_string(),
            1 => "Sleeve bottom band".to_string(),
            2 => "Sleeve top band".to_string(),
            _ => format!("Sleeve strut {}", k - 2),
        };
        parts.push((name, m));
    }
    let mut triangles: usize = parts.iter().map(|(_, m)| m.tris.len()).sum();
    let mut volume_mm3: f64 = parts.iter().map(|(_, m)| m.volume()).sum();
    let (bx, by, _) = p.bed();
    let centre = |z: f32| Vec3::new(bx * 0.5, by * 0.5, z);
    write_3mf(path, "Lampshade", &parts, centre(-d.z0())).map_err(|e| e.to_string())?;

    // split shade: every upper part is its own job, standing on its cut face
    let mut part_paths = Vec::new();
    for (k, (m, c)) in upper.into_iter().zip(split_cuts(d)).enumerate() {
        let num = k + 2;
        triangles += m.tris.len();
        volume_mm3 += m.volume();
        let pp = part_path_for(path, num);
        let title = format!("Shade part {num}/{n_parts}");
        write_3mf(&pp, &title, &[(title.clone(), m)], centre(-(d.z0() + c * d.height))).map_err(|e| e.to_string())?;
        part_paths.push(pp);
    }

    // vase mode: the glue-in mount insert is a separate, normally printed job
    // (spiral vase applies to the whole plate in the slicer)
    let insert_path = match build_insert(d, 1440) {
        Some(insert) => {
            let mut named = Vec::new();
            for (k, m) in insert.into_iter().enumerate() {
                m.check_closed().map_err(|e| format!("insert mesh not watertight: {e}"))?;
                triangles += m.tris.len();
                volume_mm3 += m.volume();
                named.push((if k == 0 { "Mount insert".to_string() } else { format!("Mount insert {}", k + 1) }, m));
            }
            let ip = insert_path_for(path);
            // flange on the bed: the insert's lowest point is z = -sleeve_height
            write_3mf(&ip, "Mount insert", &named, centre(d.sleeve_height)).map_err(|e| e.to_string())?;
            Some(ip)
        }
        None => None,
    };
    Ok(ExportInfo { triangles, volume_mm3, insert_path, part_paths })
}

/// `lamp.3mf` → `lamp-part2.3mf` (same folder).
pub fn part_path_for(path: &Path, num: usize) -> std::path::PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "lampshade".into());
    path.with_file_name(format!("{stem}-part{num}.3mf"))
}

/// `lamp.3mf` → `lamp-mount-insert.3mf` (same folder).
pub fn insert_path_for(path: &Path) -> std::path::PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "lampshade".into());
    path.with_file_name(format!("{stem}-mount-insert.3mf"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn writes_valid_3mf_package() {
        let dir = std::env::temp_dir().join("lampdesigner_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.3mf");
        let info = export_project(&Project::default(), &path, false).unwrap();
        assert!(info.triangles > 1000);
        let f = File::open(&path).unwrap();
        let mut z = zip::ZipArchive::new(f).unwrap();
        let mut s = String::new();
        z.by_name("3D/3dmodel.model").unwrap().read_to_string(&mut s).unwrap();
        assert!(s.contains("<components>"));
        assert!(s.contains("unit=\"millimeter\""));
        assert!(z.by_name("[Content_Types].xml").is_ok());
        assert!(z.by_name("_rels/.rels").is_ok());
        assert!(info.insert_path.is_none());
        assert!(info.part_paths.is_empty());
    }

    #[test]
    fn split_shade_writes_one_file_per_part() {
        let dir = std::env::temp_dir().join("lampdesigner_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("split.3mf");
        let mut p = Project::default();
        p.design.height = 320.0;
        p.design.wall = 2.0;
        p.design.split_parts = 2;
        let info = export_project(&p, &path, false).unwrap();
        assert_eq!(info.part_paths, vec![dir.join("split-part2.3mf")]);
        let mut s = String::new();
        zip::ZipArchive::new(File::open(&info.part_paths[0]).unwrap())
            .unwrap()
            .by_name("3D/3dmodel.model")
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        assert!(s.contains("Shade part 2/2"));
    }

    #[test]
    fn vase_mode_writes_a_separate_insert() {
        let dir = std::env::temp_dir().join("lampdesigner_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("vase.3mf");
        let mut p = Project::default();
        p.design.vase_mode = true;
        let info = export_project(&p, &path, false).unwrap();
        let ip = info.insert_path.expect("insert file");
        assert_eq!(ip, dir.join("vase-mount-insert.3mf"));
        let mut s = String::new();
        zip::ZipArchive::new(File::open(&ip).unwrap())
            .unwrap()
            .by_name("3D/3dmodel.model")
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        assert!(s.contains("Mount insert"));
        let mut shade = String::new();
        zip::ZipArchive::new(File::open(&path).unwrap())
            .unwrap()
            .by_name("3D/3dmodel.model")
            .unwrap()
            .read_to_string(&mut shade)
            .unwrap();
        assert!(!shade.contains("Socket sleeve"), "the shade file no longer contains the sleeve");
    }
}
