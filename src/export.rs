//! 3MF export (3MF core spec). The shell and the mount are separate closed
//! meshes combined as components of one object, so Bambu Studio / Orca /
//! PrusaSlicer load a single object with several parts and union them.

use crate::geometry::{build_mount, build_shell, export_res};
use crate::mesh::Mesh;
use crate::project::Project;
use glam::Vec3;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub struct ExportInfo {
    pub triangles: usize,
    pub volume_mm3: f64,
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
    let shell = build_shell(d, nt, nz);
    shell.check_closed().map_err(|e| format!("shell mesh not watertight: {e}"))?;
    let mut parts = vec![("Shade".to_string(), shell)];
    for (k, m) in build_mount(d, 1440).into_iter().enumerate() {
        m.check_closed().map_err(|e| format!("mount mesh not watertight: {e}"))?;
        parts.push((if k == 0 { "Mount spider".to_string() } else { "Socket sleeve".to_string() }, m));
    }
    let triangles = parts.iter().map(|(_, m)| m.tris.len()).sum();
    let volume_mm3 = parts.iter().map(|(_, m)| m.volume()).sum();
    let (bx, by, _) = p.printer.volume();
    write_3mf(path, "Lampshade", &parts, Vec3::new(bx * 0.5, by * 0.5, -d.z0())).map_err(|e| e.to_string())?;
    Ok(ExportInfo { triangles, volume_mm3 })
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
    }
}
