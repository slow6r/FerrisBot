use reqwest::Client;
use serde::Deserialize;
use chrono::{Utc, TimeZone, Timelike, Datelike};
use log::error;
use std::collections::HashMap;

const OPENWEATHER_URL: &str = "https://api.openweathermap.org/data/2.5/weather";
const FORECAST_URL: &str = "https://api.openweathermap.org/data/2.5/forecast";

#[derive(Debug, Deserialize)]
struct OpenWeatherResponse {
    main: MainInfo,
    weather: Vec<WeatherInfo>,
    wind: WindInfo,
    name: String,
    dt: i64,
    clouds: CloudsInfo,
    sys: SysInfo,
    visibility: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct MainInfo {
    temp: f32,
    feels_like: f32,
    humidity: f32,
    pressure: f32,
    temp_min: f32,
    temp_max: f32,
}

#[derive(Debug, Deserialize)]
struct WeatherInfo {
    description: String,
    icon: String,
    main: String,
}

#[derive(Debug, Deserialize)]
struct WindInfo {
    speed: f32,
    deg: f32,
}

#[derive(Debug, Deserialize)]
struct CloudsInfo {
    all: i32,
}

#[derive(Debug, Deserialize)]
struct SysInfo {
    country: String,
    sunrise: i64,
    sunset: i64,
}

#[derive(Debug, Deserialize)]
struct ForecastResponse {
    list: Vec<ForecastItem>,
}

#[derive(Debug, Deserialize)]
struct ForecastItem {
    dt: i64,
    main: MainInfo,
    weather: Vec<WeatherInfo>,
    dt_txt: String,
}

#[derive(Clone)]
pub struct WeatherClient {
    client: Client,
    api_key: String,
}

impl WeatherClient {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
        }
    }

    pub async fn get_weather(&self, city: &str) -> Result<String, String> {
        let current_weather = self.fetch_current_weather(city).await?;
        let forecast = self.fetch_forecast(city).await;
        
        Ok(self.format_weather(&current_weather, forecast.ok()))
    }

    async fn fetch_current_weather(&self, city: &str) -> Result<OpenWeatherResponse, String> {
        let response = match self.client
            .get(OPENWEATHER_URL)
            .query(&[
                ("q", city),
                ("appid", &self.api_key),
                ("units", "metric"),
                ("lang", "ru"),
            ])
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                error!("Ошибка сетевого запроса погоды: {}", e);
                return Err(format!("Не удалось получить данные о погоде: {}", e));
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let error_text = match response.text().await {
                Ok(text) => text,
                Err(_) => "неизвестная ошибка".to_string(),
            };
            
            error!("Сервис погоды вернул ошибку: {} - {}", status, error_text);
            return Err(format!("Сервис погоды недоступен ({}). Возможно, указан неверный город.", status));
        }

        match response.json::<OpenWeatherResponse>().await {
            Ok(weather_data) => Ok(weather_data),
            Err(e) => {
                error!("Ошибка парсинга ответа погоды: {}", e);
                Err(format!("Не удалось обработать данные о погоде: {}", e))
            }
        }
    }

    async fn fetch_forecast(&self, city: &str) -> Result<ForecastResponse, String> {
        let response = match self.client
            .get(FORECAST_URL)
            .query(&[
                ("q", city),
                ("appid", &self.api_key),
                ("units", "metric"),
                ("lang", "ru"),
                ("cnt", "24"), // получаем прогноз на 24 часа (с интервалом 3 часа)
            ])
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                error!("Ошибка сетевого запроса прогноза: {}", e);
                return Err(format!("Не удалось получить данные о прогнозе: {}", e));
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let error_text = match response.text().await {
                Ok(text) => text,
                Err(_) => "неизвестная ошибка".to_string(),
            };
            
            error!("Сервис прогноза вернул ошибку: {} - {}", status, error_text);
            return Err(format!("Сервис прогноза недоступен ({})", status));
        }

        match response.json::<ForecastResponse>().await {
            Ok(forecast_data) => Ok(forecast_data),
            Err(e) => {
                error!("Ошибка парсинга ответа прогноза: {}", e);
                Err(format!("Не удалось обработать данные о прогнозе: {}", e))
            }
        }
    }

    pub async fn get_weekly_forecast(&self, city: &str) -> Result<String, String> {
        let forecast = self.fetch_forecast_extended(city).await?;
        Ok(self.format_weekly_forecast(&forecast))
    }

    async fn fetch_forecast_extended(&self, city: &str) -> Result<ForecastResponse, String> {
        let response = match self.client
            .get(FORECAST_URL)
            .query(&[
                ("q", city),
                ("appid", &self.api_key),
                ("units", "metric"),
                ("lang", "ru"),
                ("cnt", "40"), // получаем прогноз на 5 дней с 3-часовым интервалом (максимум 40)
            ])
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                error!("Ошибка сетевого запроса прогноза: {}", e);
                return Err(format!("Не удалось получить данные о прогнозе: {}", e));
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let error_text = match response.text().await {
                Ok(text) => text,
                Err(_) => "неизвестная ошибка".to_string(),
            };
            
            error!("Сервис прогноза вернул ошибку: {} - {}", status, error_text);
            return Err(format!("Сервис прогноза недоступен ({})", status));
        }

        match response.json::<ForecastResponse>().await {
            Ok(forecast_data) => Ok(forecast_data),
            Err(e) => {
                error!("Ошибка парсинга ответа прогноза: {}", e);
                Err(format!("Не удалось обработать данные о прогнозе: {}", e))
            }
        }
    }

    fn format_weather(&self, data: &OpenWeatherResponse, forecast: Option<ForecastResponse>) -> String {
        // Получаем эмодзи на основе иконки погоды
        let weather_emoji = self.get_weather_emoji(&data.weather[0].icon);
        
        // Получаем красивое описание направления ветра
        let wind_direction = self.get_wind_direction(data.wind.deg);
        
        // Переводим время восхода и заката в удобный формат
        let sunrise = Utc.timestamp_opt(data.sys.sunrise, 0).unwrap();
        let sunset = Utc.timestamp_opt(data.sys.sunset, 0).unwrap();
        
        // Форматирование времени
        let sunrise_time = format!("{:02}:{:02}", sunrise.hour(), sunrise.minute());
        let sunset_time = format!("{:02}:{:02}", sunset.hour(), sunset.minute());
        
        // Рекомендации по одежде
        let clothing_recommendation = self.get_clothing_recommendation(data.main.temp, data.weather[0].main.as_str());
        
        // Получаем температуры на разное время суток
        let temp_by_time = if let Some(forecast_data) = forecast {
            self.extract_temperatures_by_time(&forecast_data)
        } else {
            "Нет данных".to_string()
        };
        
        format!(
            "{} *{}*\n\n\
            🌡 *Температура:* {:.1}°C (ощущается как {:.1}°C)\n\
            {} \n\
            🔸 Мин: {:.1}°C, Макс: {:.1}°C\n\
            💧 *Влажность:* {}%\n\
            🍃 *Ветер:* {:.1} м/с, направление: {}\n\
            ☁️ *Облачность:* {}%\n\
            👁 *Видимость:* {} км\n\
            🌅 *Восход солнца:* {}\n\
            🌇 *Закат солнца:* {}\n\n\
            *Рекомендация:* {}",
            weather_emoji,
            self.capitalize_first_letter(&data.weather[0].description),
            data.main.temp,
            data.main.feels_like,
            temp_by_time,
            data.main.temp_min,
            data.main.temp_max,
            data.main.humidity,
            data.wind.speed,
            wind_direction,
            data.clouds.all,
            data.visibility.unwrap_or(0) / 1000,
            sunrise_time,
            sunset_time,
            clothing_recommendation
        )
    }
    
    fn extract_temperatures_by_time(&self, forecast: &ForecastResponse) -> String {
        if forecast.list.is_empty() {
            return "Нет данных о прогнозе".to_string();
        }

        // Определяем утро (6-11), день (12-17), вечер (18-23)
        let mut morning_temp: Option<f32> = None;
        let mut day_temp: Option<f32> = None;
        let mut evening_temp: Option<f32> = None;

        for item in &forecast.list {
            let time = Utc.timestamp_opt(item.dt, 0).unwrap();
            let hour = time.hour();

            if (6..12).contains(&hour) && morning_temp.is_none() {
                morning_temp = Some(item.main.temp);
            } else if (12..18).contains(&hour) && day_temp.is_none() {
                day_temp = Some(item.main.temp);
            } else if (18..24).contains(&hour) && evening_temp.is_none() {
                evening_temp = Some(item.main.temp);
            }

            // Если собрали все температуры, выходим из цикла
            if morning_temp.is_some() && day_temp.is_some() && evening_temp.is_some() {
                break;
            }
        }

        format!(
            "🕒 *Прогноз на сегодня:* Утро: {}, День: {}, Вечер: {}",
            morning_temp.map_or("Н/Д".to_string(), |t| format!("{:.1}°C", t)),
            day_temp.map_or("Н/Д".to_string(), |t| format!("{:.1}°C", t)),
            evening_temp.map_or("Н/Д".to_string(), |t| format!("{:.1}°C", t))
        )
    }
    
    fn get_weather_emoji(&self, icon: &str) -> &'static str {
        match icon {
            "01d" => "☀️",  // ясно (день)
            "01n" => "🌙",  // ясно (ночь)
            "02d" => "🌤️", // малооблачно (день)
            "02n" => "🌙☁️", // малооблачно (ночь)
            "03d" | "03n" => "☁️", // облачно
            "04d" | "04n" => "☁️☁️", // пасмурно
            "09d" | "09n" => "🌧️", // дождь
            "10d" => "🌦️", // дождь с прояснениями (день)
            "10n" => "🌧️🌙", // дождь с прояснениями (ночь)
            "11d" | "11n" => "⛈️", // гроза
            "13d" | "13n" => "❄️", // снег
            "50d" | "50n" => "🌫️", // туман
            _ => "🌡️",
        }
    }
    
    fn get_wind_direction(&self, degrees: f32) -> &'static str {
        let directions = [
            "северный", "северо-восточный", "восточный", "юго-восточный",
            "южный", "юго-западный", "западный", "северо-западный"
        ];
        
        let index = ((degrees + 22.5) % 360.0 / 45.0) as usize;
        directions[index]
    }
    
    fn get_clothing_recommendation(&self, temp: f32, weather_main: &str) -> String {
        if temp < -15.0 {
            "Очень холодно! Нужна теплая зимняя одежда, шапка, шарф и теплые перчатки.".to_string()
        } else if temp < -5.0 {
            "Холодно. Наденьте зимнюю куртку, шапку и перчатки.".to_string()
        } else if temp < 5.0 {
            "Прохладно. Понадобится осенняя/зимняя куртка и шапка.".to_string()
        } else if temp < 15.0 {
            if weather_main == "Rain" {
                "Прохладно и дождливо. Возьмите зонт и непромокаемую куртку.".to_string()
            } else {
                "Прохладно. Легкая куртка или свитер будут в самый раз.".to_string()
            }
        } else if temp < 22.0 {
            if weather_main == "Rain" {
                "Тепло, но дождливо. Не забудьте зонт!".to_string()
            } else {
                "Тепло. Легкая одежда, возможно легкая кофта вечером.".to_string()
            }
        } else if temp < 30.0 {
            "Жарко. Легкая летняя одежда, не забудьте головной убор и солнцезащитный крем.".to_string()
        } else {
            "Очень жарко! Носите минимум одежды, пейте больше воды и используйте солнцезащитный крем.".to_string()
        }
    }
    
    fn capitalize_first_letter(&self, s: &str) -> String {
        let mut chars = s.chars();
        match chars.next() {
            None => String::new(),
            Some(first) => first.to_uppercase().chain(chars).collect(),
        }
    }

    fn format_weekly_forecast(&self, forecast: &ForecastResponse) -> String {
        if forecast.list.is_empty() {
            return "Нет данных о прогнозе".to_string();
        }

        // Группируем прогноз по дням
        let mut days_forecast: HashMap<String, (String, Vec<&ForecastItem>)> = HashMap::new();
        
        for item in &forecast.list {
            // Используем формат даты из dt_txt: "2023-11-21 15:00:00"
            // Получаем только дату (первые 10 символов)
            let date_str = if item.dt_txt.len() >= 10 {
                item.dt_txt[0..10].to_string()
            } else {
                // Запасной вариант, если dt_txt имеет неожиданный формат
                let date = Utc.timestamp_opt(item.dt, 0).unwrap();
                date.format("%Y-%m-%d").to_string()
            };
            
            // Определяем день недели
            let date = Utc.timestamp_opt(item.dt, 0).unwrap();
            let day_name = match date.weekday() {
                chrono::Weekday::Mon => "Понедельник",
                chrono::Weekday::Tue => "Вторник",
                chrono::Weekday::Wed => "Среда",
                chrono::Weekday::Thu => "Четверг",
                chrono::Weekday::Fri => "Пятница",
                chrono::Weekday::Sat => "Суббота",
                chrono::Weekday::Sun => "Воскресенье",
            };
            
            // Добавляем прогноз в соответствующий день
            days_forecast.entry(date_str)
                .or_insert_with(|| (day_name.to_string(), Vec::new()))
                .1.push(item);
        }

        // Форматируем прогноз для каждого дня
        let mut result = String::new();
        
        // Сортируем дни
        let mut days: Vec<(String, (String, Vec<&ForecastItem>))> = days_forecast.into_iter().collect();
        days.sort_by(|a, b| a.0.cmp(&b.0));
        
        for (date, (day_name, forecasts)) in days {
            // Обрабатываем данные для дня
            let mut min_temp = f32::MAX;
            let mut max_temp = f32::MIN;
            let mut descriptions = Vec::new();
            
            for item in &forecasts {
                min_temp = min_temp.min(item.main.temp_min);
                max_temp = max_temp.max(item.main.temp_max);
                
                if let Some(weather_info) = item.weather.first() {
                    descriptions.push(self.capitalize_first_letter(&weather_info.description));
                }
            }
            
            // Убираем дубликаты в описаниях
            descriptions.sort();
            descriptions.dedup();
            
            // Добавляем прогноз для дня - форматируем дату как день.месяц
            let date_parts: Vec<&str> = date.split('-').collect();
            let formatted_date = if date_parts.len() >= 3 {
                format!("{}.{}", date_parts[2], date_parts[1]) // день.месяц
            } else {
                date.clone() // в случае ошибки берем исходную строку
            };
            
            result.push_str(&format!("*{}, {}*:\n", day_name, formatted_date));
            result.push_str(&format!("🌡 Температура: {:.1}°C — {:.1}°C\n", min_temp, max_temp));
            result.push_str(&format!("🌤 Погода: {}\n\n", descriptions.join(", ")));
        }
        
        result
    }
}