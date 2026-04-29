use crate::toast::Toast;
use eframe::egui::{CentralPanel, Image, ScrollArea};

const BTC_QR: &[u8] = include_bytes!("../../assets/crypto/btcQR.jpeg");
const ETH_QR: &[u8] = include_bytes!("../../assets/crypto/ethQR.jpeg");
const SOL_QR: &[u8] = include_bytes!("../../assets/crypto/solQR.jpeg");
const LTC_QR: &[u8] = include_bytes!("../../assets/crypto/ltcQR.jpeg");

fn load_qr_texture(
    ctx: &eframe::egui::Context,
    name: &str,
    bytes: &[u8],
) -> eframe::egui::TextureHandle {
    if let Some(existing) =
        ctx.data(|d| d.get_temp::<eframe::egui::TextureHandle>(eframe::egui::Id::new(name)))
    {
        return existing;
    }
    let image = image::load_from_memory(bytes)
        .expect("Failed to load QR image")
        .to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let pixel_data = image.into_raw();
    let texture = ctx.load_texture(
        name,
        eframe::egui::ColorImage::from_rgba_unmultiplied(size, &pixel_data),
        eframe::egui::TextureOptions::default(),
    );
    ctx.data_mut(|d| d.insert_temp(eframe::egui::Id::new(name), texture.clone()));
    texture
}

pub fn show_donate_tab(ctx: &eframe::egui::Context, toasts: &mut crate::toast::Toasts) {
    CentralPanel::default().show(ctx, |ui| {
        ScrollArea::vertical().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.heading("💰 Support Development");
                ui.add_space(16.0);
                ui.label("If you find this app useful, consider donating!");
                ui.add_space(24.0);
                ui.separator();
                ui.add_space(16.0);
                ui.heading("Cryptocurrency");
                ui.add_space(12.0);

                ui.label("🔴 BTC");
                ui.monospace("bc1qf2vq9y7ahpkxjlrscaydwlau6t38qttt8fw3j9");
                ui.add_space(4.0);
                let btc_texture = load_qr_texture(ctx, "btc_qr", BTC_QR);
                ui.add(Image::new(&btc_texture).max_width(150.0));
                ui.add_space(4.0);
                if ui.button("📋 Copy").clicked() {
                    ui.output_mut(|o| {
                        o.copied_text = "bc1qf2vq9y7ahpkxjlrscaydwlau6t38qttt8fw3j9".to_string()
                    });
                    toasts.push(Toast::success("BTC address copied!"));
                }

                ui.add_space(16.0);
                ui.label("🔵 ETH");
                ui.monospace("0x07aB125E66DA622B9AcFA20C5797bb7C9b05e647");
                ui.add_space(4.0);
                let eth_texture = load_qr_texture(ctx, "eth_qr", ETH_QR);
                ui.add(Image::new(&eth_texture).max_width(150.0));
                ui.add_space(4.0);
                if ui.button("📋 Copy").clicked() {
                    ui.output_mut(|o| {
                        o.copied_text = "0x07aB125E66DA622B9AcFA20C5797bb7C9b05e647".to_string()
                    });
                    toasts.push(Toast::success("ETH address copied!"));
                }

                ui.add_space(16.0);
                ui.label("🟣 SOL");
                ui.monospace("ETdx5cpkM7BMnGq8zULou2Y7idp7SNf2aXmig52X7GJ2");
                ui.add_space(4.0);
                let sol_texture = load_qr_texture(ctx, "sol_qr", SOL_QR);
                ui.add(Image::new(&sol_texture).max_width(150.0));
                ui.add_space(4.0);
                if ui.button("📋 Copy").clicked() {
                    ui.output_mut(|o| {
                        o.copied_text = "ETdx5cpkM7BMnGq8zULou2Y7idp7SNf2aXmig52X7GJ2".to_string()
                    });
                    toasts.push(Toast::success("SOL address copied!"));
                }

                ui.add_space(16.0);
                ui.label("🟡 LTC");
                ui.monospace("ltc1qrvd6kxsx65zpz4et4xzmq4qdvwj86m9xmd43c8");
                ui.add_space(4.0);
                let ltc_texture = load_qr_texture(ctx, "ltc_qr", LTC_QR);
                ui.add(Image::new(&ltc_texture).max_width(150.0));
                ui.add_space(4.0);
                if ui.button("📋 Copy").clicked() {
                    ui.output_mut(|o| {
                        o.copied_text = "ltc1qrvd6kxsx65zpz4et4xzmq4qdvwj86m9xmd43c8".to_string()
                    });
                    toasts.push(Toast::success("LTC address copied!"));
                }

                ui.add_space(24.0);
                ui.separator();
                ui.add_space(16.0);
                ui.label("Thank you for your support! ❤️");
            });
        });
    });
}
