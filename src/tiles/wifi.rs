use mecha_wayland::prelude::*;

use super::{build_label, build_tile};
use crate::services::{WifiChanged, WifiStatus};

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
        let icon = build_tile(me, s, include_bytes!("../../assets/wifi/wifi-high.svg"));
        let name = build_label(me, s, b.font, "");
        s.on::<WifiChanged>(me, move |ctx, _| {
            let label = {
                let wifi = ctx.resource::<WifiStatus>();
                wifi.snapshot
                    .connections
                    .iter()
                    .find_map(|device| {
                        device.access_point.as_ref()?;
                        let connection = device.connection.as_ref()?;
                        Some(String::from_utf8_lossy(&connection.ssid).into_owned())
                    })
                    .unwrap_or_default()
            };

            ctx.at(name).unwrap().set_text(label);
        });
        Wifi { icon, name }
    }
}
