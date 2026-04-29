use eframe::egui::{CentralPanel, ScrollArea};

const LOGO_BYTES: &[u8] = include_bytes!("../../../assets/Logo.png");

fn load_logo_texture(ctx: &eframe::egui::Context) -> Option<eframe::egui::TextureHandle> {
    if let Some(existing) =
        ctx.data(|d| d.get_temp::<eframe::egui::TextureHandle>(eframe::egui::Id::new("about_logo")))
    {
        return Some(existing);
    }

    let img = image::load_from_memory(LOGO_BYTES).ok()?;
    let rgba_img = img.to_rgba8();
    let (w, h) = rgba_img.dimensions();
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for p in rgba_img.pixels() {
        rgba.push(p[0]);
        rgba.push(p[1]);
        rgba.push(p[2]);
        rgba.push(p[3]);
    }

    let texture = ctx.load_texture(
        "about_logo",
        eframe::egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
        eframe::egui::TextureOptions::default(),
    );

    ctx.data_mut(|d| d.insert_temp(eframe::egui::Id::new("about_logo"), texture.clone()));

    Some(texture)
}

pub fn show_about_tab(ctx: &eframe::egui::Context) {
    CentralPanel::default().show(ctx, |ui| {
        ScrollArea::vertical().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);

                if let Some(texture) = load_logo_texture(ui.ctx()) {
                    ui.image((texture.id(), eframe::egui::Vec2::new(128.0, 128.0)));
                }

                ui.add_space(16.0);
                ui.heading("VH Roblox Manager");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));

                ui.add_space(20.0);
                ui.separator();
                ui.add_space(10.0);

                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label("Author:");
                        ui.hyperlink_to("Varatos99", "https://discord.gg/PLACEHOLDER");
                    });

                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.label("Honorable mention:");
                        ui.hyperlink_to(
                            "centerepic",
                            "https://gitlab.com/centerepic/robloxmanager",
                        );
                    });

                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.label("Repository:");
                        ui.hyperlink_to(
                            "GitHub (coming soon)",
                            "https://github.com/USERNAME/VHRobloxManager",
                        );
                    });
                });

                ui.add_space(20.0);

                ui.separator();
                ui.add_space(10.0);
                ui.label("A Roblox account manager that lets you:");
                ui.label("• Manage multiple Roblox accounts");
                ui.label("• Launch games with different accounts");
                ui.label("• Save and manage private servers");
                ui.label("• Auto-login with browser (cookie capture)");
            });
        });
    });
}
