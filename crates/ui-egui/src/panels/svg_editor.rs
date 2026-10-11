//! Native SVG title editor. The document is deliberately limited to the safe SVG subset exposed
//! by `filmcraft-svg`; unsupported imported constructs are reported rather than executed.
use egui::{Color32, RichText};
use filmcraft_svg::{Color, Shape, ShapeKind, SvgDocument};
use serde_json::json;

const STATE: &str = "filmcraft.svg-editor.document";
const OPEN: &str = "filmcraft.svg-editor.open";
const SOURCE: &str = "filmcraft.svg-editor.source";
const PREVIEW_DOC: &str = "filmcraft.svg-editor.preview-doc";
const PREVIEW_TEXTURE: &str = "filmcraft.svg-editor.preview-texture";
const EDITING_EXISTING: &str = "filmcraft.svg-editor.editing-existing";

pub fn open(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        d.insert_temp(egui::Id::new(STATE), SvgDocument::default());
        d.insert_temp::<Option<String>>(egui::Id::new(SOURCE), None);
        d.insert_temp(egui::Id::new(EDITING_EXISTING), false);
        d.insert_temp(egui::Id::new(OPEN), true);
    });
    ctx.request_repaint();
}

/// Open a previously saved title from disk. Returns a warning summary when the file contains
/// SVG constructs outside FilmCraft's intentionally bounded subset.
pub fn open_existing(ctx: &egui::Context, path: &str) -> Result<(), String> {
    let source = std::fs::read_to_string(path).map_err(|e| format!("Could not read SVG: {e}"))?;
    let (doc, warnings) = SvgDocument::parse(&source).map_err(|e| format!("Could not parse SVG: {e}"))?;
    ctx.data_mut(|d| {
        d.insert_temp(egui::Id::new(STATE), doc);
        d.insert_temp(egui::Id::new(SOURCE), Some(path.to_owned()));
        d.insert_temp(egui::Id::new(EDITING_EXISTING), true);
        d.insert_temp(egui::Id::new(OPEN), true);
    });
    ctx.request_repaint();
    if warnings.is_empty() { Ok(()) }
    else { Err(format!("Opened SVG with unsupported constructs ignored: {}", warnings.iter().map(|w| w.0.as_str()).collect::<Vec<_>>().join("; "))) }
}

pub fn show(app: &mut crate::FilmcraftApp, ctx: &egui::Context) {
    let open = ctx.data(|d| d.get_temp::<bool>(egui::Id::new(OPEN))).unwrap_or(false);
    if !open { return; }
    let mut doc = ctx.data(|d| d.get_temp::<SvgDocument>(egui::Id::new(STATE))).unwrap_or_default();
    let source_path = ctx.data(|d| d.get_temp::<Option<String>>(egui::Id::new(SOURCE))).flatten();
    let editing_existing = ctx.data(|d| d.get_temp::<bool>(egui::Id::new(EDITING_EXISTING))).unwrap_or(false);
    let mut keep_open = true;
    let mut save = false;
    let mut open_svg = false;
    let mut close_requested = false;
    egui::Window::new("New Legacy Title (SVG)")
        .id(egui::Id::new("legacy-title-svg-editor"))
        .open(&mut keep_open).collapsible(false).resizable(true)
        .default_size([760.0, 570.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Canvas");
                ui.add(egui::DragValue::new(&mut doc.width).range(1..=8192).prefix("W "));
                ui.add(egui::DragValue::new(&mut doc.height).range(1..=8192).prefix("H "));
                if ui.button("Open SVG…").clicked() { open_svg = true; }
                if ui.button("Add rectangle").clicked() {
                    doc.shapes.push(Shape { kind: ShapeKind::Rect { x: doc.width as f32 * 0.25, y: doc.height as f32 * 0.25, width: doc.width as f32 * 0.5, height: doc.height as f32 * 0.5 }, fill: Some(Color(255,255,255,255)), stroke: None, stroke_width: 1.0, opacity: 1.0 });
                }
                if ui.button("Add ellipse").clicked() {
                    doc.shapes.push(Shape { kind: ShapeKind::Ellipse { cx: doc.width as f32 * 0.5, cy: doc.height as f32 * 0.5, rx: doc.width as f32 * 0.2, ry: doc.height as f32 * 0.2 }, fill: Some(Color(255,220,40,255)), stroke: None, stroke_width: 1.0, opacity: 1.0 });
                }
            });
            ui.separator();
            ui.columns(2, |cols| {
                cols[0].heading("Objects");
                if doc.shapes.is_empty() { cols[0].label("No objects yet. Add a rectangle or ellipse."); }
                let mut remove = None;
                for (i, shape) in doc.shapes.iter_mut().enumerate() {
                    egui::CollapsingHeader::new(format!("{} {}", i + 1, match &shape.kind { ShapeKind::Rect {..} => "Rectangle", ShapeKind::Ellipse {..} => "Ellipse", ShapeKind::Path {..} => "Path" }))
                        .id_salt(("svg-shape", i)).show(&mut cols[0], |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Fill RGBA");
                                if let Some(c) = shape.fill.as_mut() {
                                    ui.add(egui::DragValue::new(&mut c.0).range(0..=255)); ui.add(egui::DragValue::new(&mut c.1).range(0..=255)); ui.add(egui::DragValue::new(&mut c.2).range(0..=255)); ui.add(egui::DragValue::new(&mut c.3).range(0..=255));
                                } else if ui.button("Enable fill").clicked() { shape.fill = Some(Color(255,255,255,255)); }
                            });
                            ui.horizontal(|ui| { ui.label("Opacity"); ui.add(egui::Slider::new(&mut shape.opacity, 0.0..=1.0)); ui.label("Stroke width"); ui.add(egui::DragValue::new(&mut shape.stroke_width).range(0.0..=100.0)); });
                            ui.horizontal(|ui| {
                                ui.label("Stroke RGBA");
                                if let Some(c) = shape.stroke.as_mut() {
                                    ui.add(egui::DragValue::new(&mut c.0).range(0..=255)); ui.add(egui::DragValue::new(&mut c.1).range(0..=255)); ui.add(egui::DragValue::new(&mut c.2).range(0..=255)); ui.add(egui::DragValue::new(&mut c.3).range(0..=255));
                                    if ui.small_button("Disable").clicked() { shape.stroke = None; }
                                } else if ui.button("Enable stroke").clicked() { shape.stroke = Some(Color(0,0,0,255)); }
                            });
                            match &mut shape.kind {
                                ShapeKind::Rect {x,y,width,height} => { ui.add(egui::DragValue::new(x).prefix("x ")); ui.add(egui::DragValue::new(y).prefix("y ")); ui.add(egui::DragValue::new(width).prefix("w ").range(0.0..=16384.0)); ui.add(egui::DragValue::new(height).prefix("h ").range(0.0..=16384.0)); }
                                ShapeKind::Ellipse {cx,cy,rx,ry} => { ui.add(egui::DragValue::new(cx).prefix("cx ")); ui.add(egui::DragValue::new(cy).prefix("cy ")); ui.add(egui::DragValue::new(rx).prefix("rx ").range(0.0..=16384.0)); ui.add(egui::DragValue::new(ry).prefix("ry ").range(0.0..=16384.0)); }
                                ShapeKind::Path {commands} => { ui.label("Path commands"); ui.text_edit_singleline(commands); }
                            }
                            if ui.button("Remove object").clicked() { remove = Some(i); }
                        });
                }
                if let Some(i) = remove { if i < doc.shapes.len() { doc.shapes.remove(i); } }
                cols[1].heading("Preview");
                let available = cols[1].available_size();
                let scale = (available.x / doc.width.max(1) as f32).min((available.y - 24.0).max(1.0) / doc.height.max(1) as f32).max(0.001);
                let size = egui::vec2(doc.width as f32 * scale, doc.height as f32 * scale);
                let (rect, _) = cols[1].allocate_exact_size(size, egui::Sense::hover());
                cols[1].painter().rect_filled(rect, 0.0, Color32::from_gray(35));
                // Use the same bounded rasterizer as export so imported paths and strokes are
                // previewed faithfully. Cache by serialized document to avoid rasterizing every frame.
                let preview_key = serde_json::to_string(&doc).unwrap_or_default();
                let cached_key = ctx.data(|d| d.get_temp::<String>(egui::Id::new(PREVIEW_DOC))).unwrap_or_default();
                if preview_key != cached_key {
                    if let Ok(rgba) = doc.rasterize() {
                        let color_image = egui::ColorImage::from_rgba_unmultiplied([doc.width as usize, doc.height as usize], &rgba);
                        let texture = ctx.load_texture("legacy-title-svg-preview", color_image, egui::TextureOptions::LINEAR);
                        ctx.data_mut(|d| { d.insert_temp(egui::Id::new(PREVIEW_TEXTURE), texture); d.insert_temp(egui::Id::new(PREVIEW_DOC), preview_key.clone()); });
                    }
                }
                if let Some(texture) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(egui::Id::new(PREVIEW_TEXTURE))) {
                    cols[1].painter().image(texture.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Save SVG + create title still").strong()).clicked() { save = true; }
                if ui.button("Close").clicked() { close_requested = true; }
            });
        });
    if close_requested { keep_open = false; }
    ctx.data_mut(|d| { d.insert_temp(egui::Id::new(STATE), doc.clone()); d.insert_temp(egui::Id::new(OPEN), keep_open); });
    if open_svg {
        let path = app.hooks.pick_open_file.as_mut().and_then(|pick| pick("SVG image", &["svg"]));
        if let Some(path) = path {
            match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|source| filmcraft_svg::SvgDocument::parse(&source).map_err(|e| e.to_string())) {
                Ok((loaded, warnings)) => {
                    doc = loaded;
                    ctx.data_mut(|d| { d.insert_temp(egui::Id::new(STATE), doc.clone()); d.insert_temp(egui::Id::new(SOURCE), Some(path.clone())); d.insert_temp(egui::Id::new(EDITING_EXISTING), false); });
                    if warnings.is_empty() { app.ui.status = format!("Opened SVG: {}", path); }
                    else { app.ui.status = format!("Opened SVG with warnings: {}", warnings.iter().map(|w| w.0.as_str()).collect::<Vec<_>>().join("; ")); }
                }
                Err(e) => app.ui.status = format!("Could not open SVG: {e}"),
            }
        }
    }
    if save {
        let svg = match doc.to_svg() { Ok(s) => s, Err(e) => { app.ui.status = format!("SVG export failed: {e}"); return; } };
        let suggested = source_path.as_deref().and_then(|p| std::path::Path::new(p).file_name()).and_then(|n| n.to_str()).unwrap_or("Legacy Title.svg").to_owned();
        let path = app.hooks.pick_save_as.as_mut().and_then(|pick| pick("SVG image", &["svg"], &suggested));
        if let Some(path) = path {
            match std::fs::write(&path, svg.as_bytes()) {
                Err(e) => app.ui.status = format!("Could not save SVG: {e}"),
                Ok(()) => {
                    ctx.data_mut(|d| d.insert_temp(egui::Id::new(SOURCE), Some(path.clone())));
                    let png_path = std::path::Path::new(&path).with_extension("png");
                    match doc.rasterize().map_err(|e| e.to_string()).and_then(|rgba| {
                        let image = image::RgbaImage::from_raw(doc.width, doc.height, rgba)
                            .ok_or_else(|| "could not create PNG image buffer".to_string())?;
                        image.save(&png_path).map_err(|e| e.to_string())
                    }) {
                        Err(e) => app.ui.status = format!("SVG saved, but PNG rendering failed: {e}"),
                        Ok(()) => match app.session.execute("file.import", json!({"paths":[png_path.to_string_lossy()]})) {
                            Err(e) => app.ui.status = format!("SVG and PNG saved, but importing the still failed: {e}"),
                            Ok(imported) => {
                                let item = imported.get("items").and_then(serde_json::Value::as_array).and_then(|a| a.first()).and_then(serde_json::Value::as_u64);
                                if let Some(item) = item {
                                    if editing_existing {
                                        match app.session.execute("clip.replaceFromBin", json!({"item":item})) {
                                            Ok(_) => {
                                                app.ui.status = format!("Updated the selected Legacy Title from {}", png_path.display());
                                                ctx.data_mut(|d| d.insert_temp(egui::Id::new(OPEN), false));
                                            }
                                            Err(e) => app.ui.status = format!("SVG saved and PNG imported, but replacing the selected title failed: {e}"),
                                        }
                                    } else {
                                        if let Err(e) = app.session.execute("file.newSequence", json!({"name":"Legacy Title","width":doc.width,"height":doc.height,"video":1,"audio":0})) {
                                            app.ui.status = format!("SVG and PNG saved, but creating the title sequence failed: {e}");
                                            return;
                                        }
                                        match app.session.execute("timeline.place", json!({"item":item,"track":"V1","time":0})) {
                                            Ok(_) => {
                                                app.ui.status = format!("Saved SVG, rendered PNG, imported it and placed the title at the start of V1: {}", png_path.display());
                                                ctx.data_mut(|d| d.insert_temp(egui::Id::new(OPEN), false));
                                            }
                                            Err(e) => app.ui.status = format!("SVG saved and PNG imported, but timeline placement failed: {e}"),
                                        }
                                    }
                                } else {
                                    app.ui.status = format!("SVG saved and PNG imported, but the imported media ID was not returned: {imported}");
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
