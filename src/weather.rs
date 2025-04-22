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
    clouds: CloudsInfo,
    sys: SysInfo,
    visibility: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct MainInfo {
    temp: f32,
    feels_like: f32,
    humidity: f32,
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
        
        // Предварительно форматируем числа с плавающей точкой, 
        // чтобы экранировать все точки в них перед вставкой в строку
        let temp = format!("{:.1}", data.main.temp).replace(".", "\\.");
        let feels_like = format!("{:.1}", data.main.feels_like).replace(".", "\\.");
        let temp_min = format!("{:.1}", data.main.temp_min).replace(".", "\\.");
        let temp_max = format!("{:.1}", data.main.temp_max).replace(".", "\\.");
        let wind_speed = format!("{:.1}", data.wind.speed).replace(".", "\\.");
        let visibility = format!("{}", data.visibility.unwrap_or(0) / 1000);
        
        format!(
            "{} {}\n\n\
            🌡 Температура: {}°C (ощущается как {}°C)\n\
            {} \n\
            🔸 Мин: {}°C, Макс: {}°C\n\
            💧 Влажность: {}%\n\
            🍃 Ветер: {} м/с, направление: {}\n\
            ☁️ Облачность: {}%\n\
            👁 Видимость: {} км\n\
            🌅 Восход солнца: {}\n\
            🌇 Закат солнца: {}\n\n\
            Рекомендация: {}",
            weather_emoji,
            self.capitalize_first_letter(&data.weather[0].description),
            temp,
            feels_like,
            temp_by_time,
            temp_min,
            temp_max,
            data.main.humidity,
            wind_speed,
            wind_direction,
            data.clouds.all,
            visibility,
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
            "🕒 Прогноз на сегодня: Утро: {}, День: {}, Вечер: {}",
            morning_temp.map_or("Н/Д".to_string(), |t| format!("{:.1}°C", t).replace(".", "\\.")),
            day_temp.map_or("Н/Д".to_string(), |t| format!("{:.1}°C", t).replace(".", "\\.")),
            evening_temp.map_or("Н/Д".to_string(), |t| format!("{:.1}°C", t).replace(".", "\\."))
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
        // Заменяем все числовые температуры с правильным экранированием для MarkdownV2
        let temp_str = format!("{:.1}", temp).replace(".", "\\.");
        
        if temp < -25.0 {
            format!("🥶 Крайне холодно! {}°C! Нужна очень теплая многослойная одежда: термобелье, теплый свитер, зимняя куртка/пуховик, утепленные брюки, теплая шапка, шарф, варежки/перчатки и зимняя обувь с тёплыми носками.", temp_str)
        } else if temp < -15.0 {
            format!("❄️ Очень холодно! {}°C! Наденьте теплую зимнюю куртку/пуховик, утепленные брюки, многослойную одежду (термобелье, свитер), теплую шапку, шарф, перчатки и зимнюю обувь. Не забудьте про теплые носки.", temp_str)
        } else if temp < -5.0 {
            format!("🧣 Холодно. {}°C. Необходима зимняя куртка, теплый свитер, шапка, перчатки и шарф. Лучше надеть утепленные брюки и зимнюю обувь. Если планируете долго находиться на улице, подумайте о термобелье.", temp_str)
        } else if temp < 5.0 {
            if weather_main == "Rain" || weather_main == "Drizzle" {
                format!("🌧️ Холодно и дождливо. {}°C. Наденьте теплую водонепроницаемую куртку, шапку, перчатки, шарф. Обязательно возьмите зонт или наденьте куртку с капюшоном. Рекомендуется водонепроницаемая обувь.", temp_str)
            } else if weather_main == "Snow" {
                format!("🌨️ Холодно и снежно. {}°C. Наденьте теплую зимнюю куртку, шапку, перчатки, шарф и зимнюю обувь с хорошим протектором. Возможно понадобятся утепленные брюки.", temp_str)
            } else {
                format!("🧥 Прохладно. {}°C. Понадобится теплая куртка, свитер или толстовка, шапка и перчатки. Подойдет легкая шапка и шарф, особенно при ветре.", temp_str)
            }
        } else if temp < 10.0 {
            if weather_main == "Rain" || weather_main == "Drizzle" {
                format!("🌂 Прохладно и дождливо. {}°C. Возьмите водонепроницаемую куртку или плащ, зонт и наденьте водонепроницаемую обувь. Свитер или толстовка не помешают, так как на улице довольно прохладно.", temp_str)
            } else {
                format!("🧶 Прохладно. {}°C. Подойдет легкая куртка или плотная кофта, джинсы или брюки. При сильном ветре может понадобиться шарф. Утром и вечером будет прохладнее - возьмите дополнительный слой одежды.", temp_str)
            }
        } else if temp < 15.0 {
            if weather_main == "Rain" || weather_main == "Drizzle" {
                "☔ Умеренно прохладно и дождливо. Возьмите зонт и наденьте водонепроницаемую куртку или плащ. Хорошим решением будет легкий свитер или кофта и удобная непромокаемая обувь.".to_string()
            } else {
                "👕 Умеренно прохладно. Достаточно легкой куртки или кофты, можно надеть джинсы или брюки. Если проведете весь день на улице, возьмите дополнительный слой на вечер.".to_string()
            }
        } else if temp < 20.0 {
            if weather_main == "Rain" || weather_main == "Drizzle" {
                "🌦️ Тепло, но дождливо. Возьмите зонт и легкую водонепроницаемую куртку или дождевик. Подойдет футболка и джинсы/брюки. Не забудьте про удобную непромокаемую обувь.".to_string()
            } else {
                "👚 Тепло. Достаточно футболки, рубашки или блузки, подойдут легкие брюки, джинсы или юбка. Вечером может быть прохладнее, возьмите с собой легкую кофту или кардиган.".to_string()
            }
        } else if temp < 25.0 {
            if weather_main == "Rain" || weather_main == "Drizzle" {
                "🌤️ Довольно тепло, но дождливо. Легкая одежда (футболка, шорты или легкие брюки) и зонт. Дождевик может пригодиться если дождь сильный. Обувь лучше выбрать непромокаемую.".to_string()
            } else {
                "👗 Довольно тепло. Легкая одежда: футболка, рубашка или блузка, легкие брюки, шорты или юбка. Вечером может быть прохладнее, так что кофта не помешает.".to_string()
            }
        } else if temp < 30.0 {
            if weather_main == "Rain" || weather_main == "Drizzle" {
                "🌞 Жарко, но с дождем. Максимально легкая одежда и зонтик. После дождя может быть влажно и душно - выбирайте дышащие натуральные ткани.".to_string()
            } else {
                "☀️ Жарко. Максимально легкая одежда из натуральных тканей: футболка, шорты, сарафан или легкое платье. Обязательны головной убор и солнцезащитный крем. Берегитесь прямых солнечных лучей.".to_string()
            }
        } else {
            if weather_main == "Rain" || weather_main == "Drizzle" {
                "🔥 Очень жарко, возможны дожди. Минимум самой легкой одежды из натуральных тканей. Носите светлые цвета. Зонт может пригодиться как для дождя, так и для защиты от солнца.".to_string()
            } else {
                "🔥 Очень жарко! Носите минимум самой легкой одежды из натуральных тканей, предпочтительно светлых цветов. Обязательны головной убор и солнцезащитный крем. Пейте больше воды и старайтесь находиться в тени. Избегайте активности на открытом солнце в пиковые часы.".to_string()
            }
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
            let mut avg_feels_like = 0.0;
            let mut avg_humidity = 0.0;
            let mut avg_wind_speed = 0.0;
            let mut avg_wind_deg = 0.0;
            let mut avg_clouds = 0;
            let mut main_weather_type = String::new();
            let mut main_weather_icon = String::new();
            let mut descriptions = Vec::new();
            let mut count = 0;
            
            // Агрегируем данные за день
            for item in &forecasts {
                min_temp = min_temp.min(item.main.temp_min);
                max_temp = max_temp.max(item.main.temp_max);
                avg_feels_like += item.main.feels_like;
                avg_humidity += item.main.humidity;
                
                if let Some(weather_info) = item.weather.first() {
                    descriptions.push(self.capitalize_first_letter(&weather_info.description));
                    
                    // Берем преобладающую погоду за день (средняя точка дня)
                    let time = Utc.timestamp_opt(item.dt, 0).unwrap();
                    let hour = time.hour();
                    if (12..15).contains(&hour) {
                        main_weather_type = weather_info.main.clone();
                        main_weather_icon = weather_info.icon.clone();
                    }
                }
                
                count += 1;
            }
            
            // Если не нашли данные за середину дня, берем первые доступные
            if main_weather_icon.is_empty() && !forecasts.is_empty() {
                if let Some(weather_info) = forecasts[0].weather.first() {
                    main_weather_type = weather_info.main.clone();
                    main_weather_icon = weather_info.icon.clone();
                }
            }
            
            // Вычисляем средние значения
            if count > 0 {
                avg_feels_like /= count as f32;
                avg_humidity /= count as f32;
                avg_wind_speed /= count as f32;
                avg_wind_deg /= count as f32;
                avg_clouds /= count;
            }
            
            // Получаем эмодзи на основе иконки погоды
            let weather_emoji = self.get_weather_emoji(&main_weather_icon);
            
            // Рекомендации по одежде
            let clothing_recommendation = self.get_clothing_recommendation(max_temp, &main_weather_type);
            
            // Убираем дубликаты в описаниях
            descriptions.sort();
            descriptions.dedup();
            
            // Получаем красивое описание направления ветра
            let wind_direction = self.get_wind_direction(avg_wind_deg);
            
            // Добавляем прогноз для дня - форматируем дату как день.месяц
            let date_parts: Vec<&str> = date.split('-').collect();
            let formatted_date = if date_parts.len() >= 3 {
                format!("{}.{}", date_parts[2], date_parts[1]) // день.месяц
            } else {
                date.clone() // в случае ошибки берем исходную строку
            };
            
            // Предварительно форматируем числа с плавающей точкой для безопасного MarkdownV2
            let avg_temp = format!("{:.1}", (min_temp + max_temp) / 2.0).replace(".", "\\.");
            let feels_like = format!("{:.1}", avg_feels_like).replace(".", "\\.");
            let min_temp_fmt = format!("{:.1}", min_temp).replace(".", "\\.");
            let max_temp_fmt = format!("{:.1}", max_temp).replace(".", "\\.");
            let humidity = format!("{:.0}", avg_humidity);
            let wind_speed = format!("{:.1}", avg_wind_speed).replace(".", "\\.");
            
            // Структурируем прогноз по дням как в текущей погоде, но без времен восхода/заката
            result.push_str(&format!("{} {}, {}\n\n", 
                weather_emoji,
                day_name, 
                formatted_date.replace(".", "\\.")
            ));
            
            result.push_str(&format!("🌡 Температура: {}°C (ощущается как {}°C)\n", 
                avg_temp,
                feels_like
            ));
            
            result.push_str(&format!("🔸 Мин: {}°C, Макс: {}°C\n", min_temp_fmt, max_temp_fmt));
            result.push_str(&format!("💧 Влажность: {}%\n", humidity));
            result.push_str(&format!("🍃 Ветер: {} м/с, направление: {}\n", wind_speed, wind_direction));
            result.push_str(&format!("☁️ Облачность: {}%\n", avg_clouds));
            result.push_str(&format!("🌤 Погода: {}\n", descriptions.join(", ")));
            result.push_str(&format!("Рекомендация: {}\n\n", clothing_recommendation));
        }
        
        result
    }
}