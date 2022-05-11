#![feature(duration_constants)]
use chrono::TimeZone as _;

mod config;

static COLOR_GOOD: &'static str = "#8fc029";
static COLOR_BAD: &'static str = "#dc2566";
static COLOR_UNKNOWN: &'static str = "#9358fe";
static COLOR_CHARGE: &'static str = "#e7db75";

#[derive(serde::Serialize)]
struct Block {
    full_text: String,
    color: Option<String>, // TODO: real type?
    separator: bool,
}

fn display_bar(blocks: Vec<Block>) {
    println!("{},", serde_json::to_string(&blocks).unwrap());
}

fn make_time_block(format: &str, tz: chrono_tz::Tz) -> Block {
    let utc = chrono::offset::Utc::now().naive_utc();
    let dt = tz.from_utc_datetime(&utc);
    Block {
        full_text: dt.format(format).to_string(),
        color: None,
        separator: false,
    }
}

fn get_battery_percentages(battery_manager: &battery::Manager) -> (f32, f32) {
    let mut total_energy = battery::units::Energy::new::<battery::units::energy::watt_hour>(0.0);
    let mut total_full_energy = battery::units::Energy::new::<battery::units::energy::watt_hour>(0.0);
    let mut total_design_energy = battery::units::Energy::new::<battery::units::energy::watt_hour>(0.0);
    for battery in battery_manager.batteries().unwrap() {
        if let Ok(battery) = battery {
            total_energy += battery.energy();
            total_full_energy += battery.energy_full();
            total_design_energy += battery.energy_full_design();
        }
    }

    let current_percentage = (total_energy / total_full_energy).get::<battery::units::ratio::percent>();
    let design_percentage = (total_full_energy / total_design_energy).get::<battery::units::ratio::percent>();

    (current_percentage, design_percentage)
}

// From https://github.com/nightscout/cgm-remote-monitor/blob/master/swagger.yaml
// TODO: share this with cgmserver project
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    #[serde(rename = "type")]
    type_: String,
    date_string: String,
    date: i64,
    sgv: f64,
    direction: String,
    noise: f64,
    filtered: f64,
    unfiltered: f64,
    rssi: f64,
}

fn get_battery_state(battery_manager: &battery::Manager) -> battery::State {
    let mut last_state = battery::State::Unknown;
    for battery in battery_manager.batteries().unwrap() {
        if let Ok(battery) = battery {
            let state = battery.state();
            if state != battery::State::Unknown {
                last_state = state;
            }
        }
    }
    last_state
}

fn make_battery_blocks(battery_manager: &battery::Manager) -> Vec<Block> {
    let (current_percentage, design_percentage) = get_battery_percentages(&battery_manager);
    let (state_char, color) = match (
        get_battery_state(&battery_manager),
        if current_percentage < 15.0 {
            Some(COLOR_BAD.to_string())
        } else {
            None
        },
    ) {
        (battery::State::Unknown, Some(color)) => ("?", Some(color)),
        (battery::State::Unknown, None) => ("?", Some(COLOR_UNKNOWN.to_string())),
        (battery::State::Charging, _) => ("⚡", Some(COLOR_CHARGE.to_string())),
        (battery::State::Full, _) => ("", Some(COLOR_GOOD.to_string())),
        (_, color) => ("", color),
    };
    vec![
        Block {
            full_text: format!("{}{:.1}%", state_char, current_percentage),
            color,
            separator: false,
        },
        Block {
            full_text: format!("({:.0}%)", design_percentage),
            color: None,
            separator: false,
        },
    ]
}

fn get_if_name(
    nl80211sock: &mut nl80211::Socket,
    name: String,
) -> Result<Option<String>, neli::err::NlError> {
    let mut name = name.into_bytes();
    name.push(0);
    for interface in nl80211sock
        .get_interfaces_info()?
        .iter()
        .filter(|x| x.name == Some(name.clone()))
    {
        if let Some(Ok(ssid)) = interface.ssid.clone().map(|x| String::from_utf8(x)) {
            return Ok(Some(ssid));
        }
    }
    Ok(None)
}

fn make_wifi_block(nl80211sock: &mut nl80211::Socket, name: String) -> Block {
    let (full_text, color) = match get_if_name(nl80211sock, name) {
        Ok(Some(ssid)) => (ssid, Some(COLOR_GOOD.to_string())),
        Ok(None) => ("no wifi".to_string(), Some(COLOR_UNKNOWN.to_string())),
        Err(_) => ("wifi error".to_string(), Some(COLOR_BAD.to_string())),
    };
    Block { full_text, color, separator: false }
}

// this code is very gross and should be refactored. i don't really care.
fn make_cgm_block(cgm_data: std::sync::Arc<std::sync::Mutex<Option<Entry>>>) -> Block {
    let cgm_data: Option<Entry> = cgm_data.lock().unwrap().clone();
    let (full_text, color) = match cgm_data {
        Some(cgm_data) => {
            let cgm_age = (std::time::SystemTime::now()
                     .duration_since(std::time::UNIX_EPOCH)
                     .unwrap()
                     .as_millis() - cgm_data.date as u128) / (1000 * 60);
            (
                format!(
                    "CGM {}{} ({}m)",
                    cgm_data.sgv,
                    match cgm_data.direction.as_str() {
                        "DoubleUp" => "⇈",
                        "SingleUp" => "↑",
                        "FortyFiveUp" => "➚",
                        "Flat" => "→",
                        "FortyFiveDown" => "➘",
                        "SingleDown" => "↓",
                        "DoubleDown" => "⇊",
                        _ => "",
                    },
                    cgm_age
                ),
                Some(if cgm_age > 10 {
                    COLOR_BAD
                } else if cgm_data.sgv < 70.0 {
                    COLOR_BAD
                } else if cgm_data.sgv <= 160.0 {
                    COLOR_GOOD
                } else if cgm_data.sgv > 160.0 {
                    COLOR_BAD
                } else {
                    COLOR_UNKNOWN
                }.to_string())
            )
        },
        None => (
            "CGM unknown".to_string(),
            Some(COLOR_UNKNOWN.to_string())
        ),
    };
    Block {
        full_text,
        color,
        separator: false,
    }
}

fn main() {
    let battery_manager = battery::Manager::new().unwrap();
    let mut nl80211sock = nl80211::Socket::connect().unwrap();
    let cgm_data = std::sync::Arc::new(std::sync::Mutex::new(None));
    let update_cgm_data = cgm_data.clone();

    std::thread::spawn(move || {
        loop {
            let mut data = Vec::new();
            let mut handle = curl::easy::Easy::new();
            handle.url(config::CGMSERVER_URL).unwrap();
            let mut headers = curl::easy::List::new();
            headers.append(&format!("API-Secret: {}", config::CGMSERVER_API_SECRET)).unwrap();
            handle.http_headers(headers).unwrap();

            let transfer_ok = {
                let mut transfer = handle.transfer();
                transfer.write_function(|new_data| {
                    data.extend_from_slice(new_data);
                    Ok(new_data.len())
                }).unwrap();
                transfer.perform().is_ok()
            };

            if transfer_ok {
                let entry: serde_json::Result<Entry> = serde_json::from_slice(&data);
                if let Ok(entry) = entry {
                    let mut cgm_data = update_cgm_data.lock().unwrap();
                    *cgm_data = Some(entry);
                }
            }
            // TODO: calculate how long to wait from datapoint age
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    });

    println!("{}", r#"{"version": 1}"#);
    println!("[");

    loop {
        let mut blocks = vec![];
        blocks.push(make_cgm_block(cgm_data.clone()));
        blocks.push(make_time_block("TPE %H:%M", chrono_tz::Asia::Taipei));
        blocks.push(make_time_block("SFO %H:%M", chrono_tz::America::Los_Angeles));
        blocks.push(make_time_block("NYC %H:%M", chrono_tz::America::New_York));
        blocks.push(make_wifi_block(&mut nl80211sock, "wlp3s0".to_string()));
        blocks.append(&mut make_battery_blocks(&battery_manager));

        display_bar(blocks);
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}
