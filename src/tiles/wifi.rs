use mecha_wayland::prelude::*;

use super::{build_label, build_tile_with_sprite};
use crate::load_icon;
use crate::services::{SetWifiEnabled, WifiChanged, WifiStatus};

// Inclusive lower bounds for the signal-strength icons (0–100%).
const WIFI_LOW_MIN_PERCENT: u8 = 25;
const WIFI_HIGH_MIN_PERCENT: u8 = 75;

pub struct Wifi {
    pub icon: Handle<Icon>,
    pub name: Handle<Text>,
}

pub struct WifiBuilder {
    pub font: FontId,
}

impl Build for WifiBuilder {
    type Widget = Wifi;
}

impl Widget for Wifi {
    type Builder = WifiBuilder;

    fn build(b: Self::Builder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        let [disabled, none, low, high] = [
            include_bytes!("../../assets/wifi/wifi-x.svg") as &[u8],
            include_bytes!("../../assets/wifi/wifi-none.svg"),
            include_bytes!("../../assets/wifi/wifi-low.svg"),
            include_bytes!("../../assets/wifi/wifi-high.svg"),
        ]
        .map(|svg| load_icon(&mut s.resource_mut::<Atlas>(), svg));
        let icon = build_tile_with_sprite(me, s, disabled);
        let name = build_label(me, s, b.font, "");
        s.on::<Clicked>(me, |ctx, _| {
            let enabled = ctx.resource::<WifiStatus>().snapshot.wireless_enabled;
            if let Some(enabled) = enabled {
                ctx.signal(SetWifiEnabled(!enabled));
            }
        });
        s.on::<WifiChanged>(me, move |ctx, _| {
            let (label, sprite) = {
                let wifi = ctx.resource::<WifiStatus>();
                let connection = wifi
                    .snapshot
                    .connections
                    .iter()
						  // find first connection for first device
                    .find_map(|device| {
                        device.access_point.as_ref()?;
                        device.connection.as_ref()
                    });
                let label = connection
                    .map(|connection| String::from_utf8_lossy(&connection.ssid).into_owned())
                    .unwrap_or_default();
                let sprite = if !wifi.snapshot.valid || wifi.snapshot.radio_enabled() != Some(true) {
                    disabled
                } else {
                    match connection.map(|connection| connection.strength) {
                        Some(strength) if strength >= WIFI_HIGH_MIN_PERCENT => high,
                        Some(strength) if strength >= WIFI_LOW_MIN_PERCENT => low,
                        _ => none,
                    }
                };
                (label, sprite)
            };

            ctx.at(name).unwrap().set_text(label);
            ctx.at(icon).unwrap().set_sprite(sprite);
        });
        Wifi { icon, name }
    }
}
