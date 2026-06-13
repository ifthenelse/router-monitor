use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const GEOLOCATION_URL: &str = "https://ipapi.co/json/";
const FALLBACK_GEOLOCATION_URL: &str = "https://ipwho.is/";
const OPEN_METEO_URL: &str = "https://api.open-meteo.com/v1/forecast";
const RASPBERRY_PI_THERMAL_PATH: &str = "/sys/class/thermal/thermal_zone0/temp";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Coordinates {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocationEstimate {
    pub coordinates: Coordinates,
    pub city: Option<String>,
    pub region: Option<String>,
    pub country: Option<String>,
    pub source: &'static str,
    pub precision: &'static str,
}

impl LocationEstimate {
    fn manual(coordinates: Coordinates) -> Self {
        Self {
            coordinates,
            city: None,
            region: None,
            country: None,
            source: "manual",
            precision: "manual",
        }
    }

    pub fn display_label(&self) -> String {
        let place = [
            self.city.as_deref(),
            self.region.as_deref(),
            self.country.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");

        if place.is_empty() {
            format!(
                "{:.4}, {:.4} ({}, {})",
                self.coordinates.latitude, self.coordinates.longitude, self.source, self.precision
            )
        } else {
            format!(
                "{place} ({:.4}, {:.4}; {}, {})",
                self.coordinates.latitude, self.coordinates.longitude, self.source, self.precision
            )
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EnvironmentReading {
    pub outside_temperature_c: Option<f64>,
    pub outside_relative_humidity: Option<f64>,
    pub outside_apparent_temperature_c: Option<f64>,
    pub raspberry_pi_temperature_c: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
pub struct EnvironmentConfig {
    pub location: Option<Coordinates>,
}

#[derive(Debug, Clone, Copy)]
pub struct EnvironmentSample {
    pub reading: EnvironmentReading,
    pub refreshed: bool,
}

pub trait WeatherProvider {
    fn current_conditions(&self, location: Coordinates) -> Result<EnvironmentReading>;
}

pub trait GeolocationProvider {
    fn locate(&self) -> Result<LocationEstimate>;
}

pub trait LocalEnvironmentSensor {
    fn read_temperature_c(&self) -> Option<f64>;
}

pub struct EnvironmentMonitor {
    location: Option<LocationEstimate>,
    weather_provider: Option<Box<dyn WeatherProvider>>,
    local_sensors: Vec<Box<dyn LocalEnvironmentSensor>>,
    cached: EnvironmentReading,
    last_refresh: Option<Instant>,
}

impl EnvironmentMonitor {
    pub fn new(config: EnvironmentConfig) -> Self {
        let client = match Client::builder()
            .user_agent(concat!("router-monitor/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(5))
            .build()
        {
            Ok(client) => client,
            Err(_) => return Self::disabled(),
        };

        let location = resolve_location(
            config.location,
            &CityLevelGeolocationProvider::new(client.clone()),
        );
        let weather_provider = location
            .as_ref()
            .map(|_| Box::new(OpenMeteoWeatherProvider::new(client)) as Box<dyn WeatherProvider>);

        Self {
            location,
            weather_provider,
            local_sensors: vec![Box::new(RaspberryPiTemperatureSensor::default())],
            cached: EnvironmentReading::default(),
            last_refresh: None,
        }
    }

    pub fn disabled() -> Self {
        Self {
            location: None,
            weather_provider: None,
            local_sensors: vec![Box::new(RaspberryPiTemperatureSensor::default())],
            cached: EnvironmentReading::default(),
            last_refresh: None,
        }
    }

    pub fn sample(&mut self, now: Instant) -> EnvironmentSample {
        if !self.should_refresh(now) {
            return EnvironmentSample {
                reading: self.cached,
                refreshed: false,
            };
        }

        let mut reading = self.cached;

        if let (Some(location), Some(provider)) = (&self.location, self.weather_provider.as_ref()) {
            if let Ok(outside) = provider.current_conditions(location.coordinates) {
                reading.outside_temperature_c = outside.outside_temperature_c;
                reading.outside_relative_humidity = outside.outside_relative_humidity;
                reading.outside_apparent_temperature_c = outside.outside_apparent_temperature_c;
            }
        }

        reading.raspberry_pi_temperature_c = self
            .local_sensors
            .iter()
            .find_map(|sensor| sensor.read_temperature_c());

        self.cached = reading;
        self.last_refresh = Some(now);

        EnvironmentSample {
            reading,
            refreshed: true,
        }
    }

    pub fn location(&self) -> Option<&LocationEstimate> {
        self.location.as_ref()
    }

    fn should_refresh(&self, now: Instant) -> bool {
        self.last_refresh
            .map(|last_refresh| now.duration_since(last_refresh) >= REFRESH_INTERVAL)
            .unwrap_or(true)
    }

    #[cfg(test)]
    fn with_parts(
        location: Option<LocationEstimate>,
        weather_provider: Option<Box<dyn WeatherProvider>>,
        local_sensors: Vec<Box<dyn LocalEnvironmentSensor>>,
    ) -> Self {
        Self {
            location,
            weather_provider,
            local_sensors,
            cached: EnvironmentReading::default(),
            last_refresh: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpenMeteoWeatherProvider {
    client: Client,
}

impl OpenMeteoWeatherProvider {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

impl WeatherProvider for OpenMeteoWeatherProvider {
    fn current_conditions(&self, location: Coordinates) -> Result<EnvironmentReading> {
        let url = reqwest::Url::parse_with_params(
            OPEN_METEO_URL,
            [
                ("latitude", location.latitude.to_string()),
                ("longitude", location.longitude.to_string()),
                (
                    "current",
                    "temperature_2m,relative_humidity_2m,apparent_temperature".to_string(),
                ),
            ],
        )
        .context("Cannot build Open-Meteo weather URL.")?;
        let body = self
            .client
            .get(url)
            .send()
            .context("Cannot fetch weather data from Open-Meteo.")?
            .error_for_status()
            .context("Open-Meteo returned an unsuccessful status.")?
            .text()
            .context("Cannot read Open-Meteo response.")?;
        let response: OpenMeteoResponse =
            serde_json::from_str(&body).context("Cannot parse Open-Meteo response.")?;

        Ok(EnvironmentReading {
            outside_temperature_c: Some(response.current.temperature_c),
            outside_relative_humidity: Some(response.current.relative_humidity),
            outside_apparent_temperature_c: Some(response.current.apparent_temperature_c),
            raspberry_pi_temperature_c: None,
        })
    }
}

#[derive(Debug, Deserialize)]
struct OpenMeteoResponse {
    current: OpenMeteoCurrent,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoCurrent {
    #[serde(rename = "temperature_2m")]
    temperature_c: f64,
    #[serde(rename = "relative_humidity_2m")]
    relative_humidity: f64,
    #[serde(rename = "apparent_temperature")]
    apparent_temperature_c: f64,
}

#[derive(Debug, Clone)]
pub struct IpApiGeolocationProvider {
    client: Client,
}

impl IpApiGeolocationProvider {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

impl GeolocationProvider for IpApiGeolocationProvider {
    fn locate(&self) -> Result<LocationEstimate> {
        let body = self
            .client
            .get(GEOLOCATION_URL)
            .send()
            .context("Cannot fetch public IP geolocation.")?
            .error_for_status()
            .context("Public IP geolocation returned an unsuccessful status.")?
            .text()
            .context("Cannot read public IP geolocation response.")?;
        parse_ipapi_geolocation(&body)
    }
}

#[derive(Debug, Deserialize)]
struct IpApiResponse {
    latitude: Option<f64>,
    longitude: Option<f64>,
    city: Option<String>,
    region: Option<String>,
    country_name: Option<String>,
}

fn parse_ipapi_geolocation(body: &str) -> Result<LocationEstimate> {
    let response: IpApiResponse =
        serde_json::from_str(body).context("Cannot parse public IP geolocation response.")?;
    let latitude = response
        .latitude
        .context("Public IP geolocation did not include a latitude.")?;
    let longitude = response
        .longitude
        .context("Public IP geolocation did not include a longitude.")?;

    Ok(LocationEstimate {
        coordinates: Coordinates {
            latitude,
            longitude,
        },
        city: response.city,
        region: response.region,
        country: response.country_name,
        source: "ipapi.co",
        precision: "city-level",
    })
}

#[derive(Debug, Clone)]
pub struct IpWhoIsGeolocationProvider {
    client: Client,
}

impl IpWhoIsGeolocationProvider {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

impl GeolocationProvider for IpWhoIsGeolocationProvider {
    fn locate(&self) -> Result<LocationEstimate> {
        let body = self
            .client
            .get(FALLBACK_GEOLOCATION_URL)
            .send()
            .context("Cannot fetch fallback public IP geolocation.")?
            .error_for_status()
            .context("Fallback public IP geolocation returned an unsuccessful status.")?
            .text()
            .context("Cannot read fallback public IP geolocation response.")?;
        parse_ipwhois_geolocation(&body)
    }
}

#[derive(Debug, Deserialize)]
struct IpWhoIsResponse {
    success: Option<bool>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    city: Option<String>,
    region: Option<String>,
    country: Option<String>,
}

fn parse_ipwhois_geolocation(body: &str) -> Result<LocationEstimate> {
    let response: IpWhoIsResponse = serde_json::from_str(body)
        .context("Cannot parse fallback public IP geolocation response.")?;

    if response.success == Some(false) {
        anyhow::bail!("Fallback public IP geolocation reported failure.");
    }

    let latitude = response
        .latitude
        .context("Fallback public IP geolocation did not include a latitude.")?;
    let longitude = response
        .longitude
        .context("Fallback public IP geolocation did not include a longitude.")?;

    Ok(LocationEstimate {
        coordinates: Coordinates {
            latitude,
            longitude,
        },
        city: response.city,
        region: response.region,
        country: response.country,
        source: "ipwho.is",
        precision: "city-level",
    })
}

#[derive(Debug, Clone)]
pub struct CityLevelGeolocationProvider {
    primary: IpApiGeolocationProvider,
    fallback: IpWhoIsGeolocationProvider,
}

impl CityLevelGeolocationProvider {
    pub fn new(client: Client) -> Self {
        Self {
            primary: IpApiGeolocationProvider::new(client.clone()),
            fallback: IpWhoIsGeolocationProvider::new(client),
        }
    }
}

impl GeolocationProvider for CityLevelGeolocationProvider {
    fn locate(&self) -> Result<LocationEstimate> {
        self.primary.locate().or_else(|_| self.fallback.locate())
    }
}

fn resolve_location(
    explicit_location: Option<Coordinates>,
    geolocation_provider: &dyn GeolocationProvider,
) -> Option<LocationEstimate> {
    if let Some(location) = explicit_location {
        return Some(LocationEstimate::manual(location));
    }

    if let Some(location) = read_cached_location() {
        return Some(location);
    }

    match geolocation_provider.locate() {
        Ok(location) => {
            let _ = write_cached_location(&location);
            Some(location)
        }
        Err(_) => None,
    }
}

fn read_cached_location() -> Option<LocationEstimate> {
    let path = location_cache_path()?;
    let content = fs::read_to_string(path).ok()?;

    let cached: CachedLocation = serde_json::from_str(&content).ok()?;

    Some(cached.into_location_estimate())
}

fn write_cached_location(location: &LocationEstimate) -> Result<()> {
    let path = location_cache_path().context("Cannot determine location cache path.")?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Cannot create router-monitor cache directory '{}'.",
                parent.display()
            )
        })?;
    }

    let content = serde_json::to_string(&CachedLocation::from(location))
        .context("Cannot serialize cached location.")?;
    fs::write(&path, content)
        .with_context(|| format!("Cannot write location cache '{}'.", path.display()))?;

    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedLocation {
    latitude: f64,
    longitude: f64,
    city: Option<String>,
    region: Option<String>,
    country: Option<String>,
    source: Option<String>,
    precision: Option<String>,
}

impl CachedLocation {
    fn into_location_estimate(self) -> LocationEstimate {
        LocationEstimate {
            coordinates: Coordinates {
                latitude: self.latitude,
                longitude: self.longitude,
            },
            city: self.city,
            region: self.region,
            country: self.country,
            source: cached_static_text(self.source, "cached"),
            precision: cached_static_text(self.precision, "city-level"),
        }
    }
}

impl From<&LocationEstimate> for CachedLocation {
    fn from(location: &LocationEstimate) -> Self {
        Self {
            latitude: location.coordinates.latitude,
            longitude: location.coordinates.longitude,
            city: location.city.clone(),
            region: location.region.clone(),
            country: location.country.clone(),
            source: Some(location.source.to_string()),
            precision: Some(location.precision.to_string()),
        }
    }
}

fn cached_static_text(value: Option<String>, default: &'static str) -> &'static str {
    match value.as_deref() {
        Some("manual") => "manual",
        Some("ipapi.co") => "ipapi.co",
        Some("ipwho.is") => "ipwho.is",
        Some("city-level") => "city-level",
        _ => default,
    }
}

fn location_cache_path() -> Option<PathBuf> {
    if let Some(cache_home) = std::env::var_os("XDG_CACHE_HOME") {
        return Some(PathBuf::from(cache_home).join("router-monitor/location.json"));
    }

    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".cache/router-monitor/location.json"))
}

#[derive(Debug, Clone)]
pub struct RaspberryPiTemperatureSensor {
    path: PathBuf,
}

impl Default for RaspberryPiTemperatureSensor {
    fn default() -> Self {
        Self {
            path: PathBuf::from(RASPBERRY_PI_THERMAL_PATH),
        }
    }
}

impl RaspberryPiTemperatureSensor {
    #[cfg(test)]
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl LocalEnvironmentSensor for RaspberryPiTemperatureSensor {
    fn read_temperature_c(&self) -> Option<f64> {
        read_raspberry_pi_temperature_c(&self.path)
    }
}

pub fn read_raspberry_pi_temperature_c(path: &Path) -> Option<f64> {
    let raw = fs::read_to_string(path).ok()?;
    let millidegrees_c = raw.trim().parse::<f64>().ok()?;

    Some(millidegrees_c / 1_000.0)
}

pub fn has_high_heat_conditions(reading: EnvironmentReading) -> bool {
    reading
        .outside_temperature_c
        .map(|temperature| temperature >= 30.0)
        .unwrap_or(false)
        || reading
            .outside_apparent_temperature_c
            .map(|temperature| temperature >= 35.0)
            .unwrap_or(false)
        || reading
            .outside_relative_humidity
            .map(|humidity| humidity >= 75.0)
            .unwrap_or(false)
}

pub fn format_temperature_field(value: Option<f64>) -> String {
    value
        .map(|temperature| format!("{temperature:.1}"))
        .unwrap_or_default()
}

pub fn format_humidity_field(value: Option<f64>) -> String {
    value
        .map(|humidity| format!("{humidity:.0}"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct FakeWeatherProvider {
        calls: Cell<u32>,
    }

    impl WeatherProvider for FakeWeatherProvider {
        fn current_conditions(&self, _location: Coordinates) -> Result<EnvironmentReading> {
            self.calls.set(self.calls.get() + 1);

            Ok(EnvironmentReading {
                outside_temperature_c: Some(31.4),
                outside_relative_humidity: Some(68.0),
                outside_apparent_temperature_c: Some(37.8),
                raspberry_pi_temperature_c: None,
            })
        }
    }

    struct FixedSensor;

    impl LocalEnvironmentSensor for FixedSensor {
        fn read_temperature_c(&self) -> Option<f64> {
            Some(54.2)
        }
    }

    #[test]
    fn caches_environment_reading_until_refresh_interval() {
        let provider = Box::new(FakeWeatherProvider {
            calls: Cell::new(0),
        });
        let mut monitor = EnvironmentMonitor::with_parts(
            Some(LocationEstimate::manual(Coordinates {
                latitude: 45.484,
                longitude: 9.204,
            })),
            Some(provider),
            vec![Box::new(FixedSensor)],
        );
        let started_at = Instant::now();

        let first = monitor.sample(started_at);
        let cached = monitor.sample(started_at + Duration::from_secs(60));
        let refreshed = monitor.sample(started_at + REFRESH_INTERVAL);

        assert!(first.refreshed);
        assert!(!cached.refreshed);
        assert!(refreshed.refreshed);
        assert_eq!(cached.reading.outside_temperature_c, Some(31.4));
        assert_eq!(cached.reading.raspberry_pi_temperature_c, Some(54.2));
    }

    #[test]
    fn detects_high_heat_conditions() {
        assert!(has_high_heat_conditions(EnvironmentReading {
            outside_temperature_c: Some(29.0),
            outside_relative_humidity: Some(60.0),
            outside_apparent_temperature_c: Some(35.0),
            raspberry_pi_temperature_c: None,
        }));
        assert!(has_high_heat_conditions(EnvironmentReading {
            outside_temperature_c: Some(20.0),
            outside_relative_humidity: Some(75.0),
            outside_apparent_temperature_c: Some(25.0),
            raspberry_pi_temperature_c: None,
        }));
        assert!(!has_high_heat_conditions(EnvironmentReading {
            outside_temperature_c: Some(20.0),
            outside_relative_humidity: Some(50.0),
            outside_apparent_temperature_c: Some(25.0),
            raspberry_pi_temperature_c: None,
        }));
    }

    #[test]
    fn parses_ipapi_coordinates() {
        let location = parse_ipapi_geolocation(
            r#"{"latitude":45.484,"longitude":9.204,"city":"Milan","region":"Lombardy","country_name":"Italy"}"#,
        )
        .unwrap();

        assert_eq!(location.coordinates.latitude, 45.484);
        assert_eq!(location.coordinates.longitude, 9.204);
        assert_eq!(location.city.as_deref(), Some("Milan"));
        assert_eq!(location.region.as_deref(), Some("Lombardy"));
        assert_eq!(location.country.as_deref(), Some("Italy"));
        assert_eq!(location.source, "ipapi.co");
        assert_eq!(location.precision, "city-level");
    }

    #[test]
    fn parses_fallback_ipwhois_coordinates() {
        let location = parse_ipwhois_geolocation(
            r#"{"success":true,"latitude":45.484,"longitude":9.204,"city":"Milan","region":"Lombardy","country":"Italy"}"#,
        )
        .unwrap();

        assert_eq!(location.coordinates.latitude, 45.484);
        assert_eq!(location.coordinates.longitude, 9.204);
        assert_eq!(location.city.as_deref(), Some("Milan"));
        assert_eq!(location.region.as_deref(), Some("Lombardy"));
        assert_eq!(location.country.as_deref(), Some("Italy"));
        assert_eq!(location.source, "ipwho.is");
        assert_eq!(location.precision, "city-level");
    }

    #[test]
    fn displays_city_level_location_label() {
        let location = LocationEstimate {
            coordinates: Coordinates {
                latitude: 45.484,
                longitude: 9.204,
            },
            city: Some("Milan".to_string()),
            region: Some("Lombardy".to_string()),
            country: Some("Italy".to_string()),
            source: "ipapi.co",
            precision: "city-level",
        };

        assert_eq!(
            location.display_label(),
            "Milan, Lombardy, Italy (45.4840, 9.2040; ipapi.co, city-level)"
        );
    }

    #[test]
    fn reads_raspberry_pi_millidegrees_as_celsius() {
        let path = unique_temp_path();
        fs::write(&path, "54210\n").unwrap();
        let sensor = RaspberryPiTemperatureSensor::new(path.clone());

        assert_eq!(sensor.read_temperature_c(), Some(54.21));

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn formats_environment_fields_for_csv() {
        assert_eq!(format_temperature_field(Some(31.44)), "31.4");
        assert_eq!(format_humidity_field(Some(68.4)), "68");
        assert_eq!(format_temperature_field(None), "");
    }

    fn unique_temp_path() -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!(
            "router-monitor-environment-test-{}-{timestamp}",
            std::process::id()
        ))
    }
}
