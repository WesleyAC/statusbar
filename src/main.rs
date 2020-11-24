use chrono::TimeZone as _;

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

fn get_battery_percentage(battery_manager: &battery::Manager) -> f32 {
    let mut total_energy = battery::units::Energy::new::<battery::units::energy::watt_hour>(0.0);
    let mut total_full_energy = battery::units::Energy::new::<battery::units::energy::watt_hour>(0.0);
    for battery in battery_manager.batteries().unwrap() {
        if let Ok(battery) = battery {
            total_energy += battery.energy();
            total_full_energy += battery.energy_full();
        }
    }

    (total_energy / total_full_energy).get::<battery::units::ratio::percent>()
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

fn make_battery_block(battery_manager: &battery::Manager) -> Block {
    let percentage = get_battery_percentage(&battery_manager);
    let (state_char, color) = match (
        get_battery_state(&battery_manager),
        if percentage < 15.0 {
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
    Block {
        full_text: format!("{}{:.1}%", state_char, percentage),
        color,
        separator: false,
    }
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

fn main() {
    let battery_manager = battery::Manager::new().unwrap();
    let mut nl80211sock = nl80211::Socket::connect().unwrap();

    println!("{}", r#"{"version": 1}"#);
    println!("[");

    loop {
        let mut blocks = vec![];
        blocks.push(make_time_block("TPE %H:%M", chrono_tz::Asia::Taipei));
        blocks.push(make_time_block("SFO %H:%M", chrono_tz::America::Los_Angeles));
        blocks.push(make_time_block("NYC %H:%M", chrono_tz::America::New_York));
        blocks.push(make_wifi_block(&mut nl80211sock, "wlp3s0".to_string()));
        blocks.push(make_battery_block(&battery_manager));

        display_bar(blocks);
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}
