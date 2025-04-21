use teloxide::types::ChatId;
use teloxide::Bot;
use super::storage::JsonStorage;
use super::weather::WeatherClient;
use chrono::{Local, Datelike, Weekday, DateTime, Timelike, Utc};
use tokio::time::{sleep, Duration};
use std::sync::Arc;
use teloxide::payloads::SendMessageSetters;
use teloxide::prelude::Requester;
use rand::Rng;
use log::{info, error, warn};

// Вспомогательная функция для экранирования специальных символов Markdown
fn escape_markdown_v2(text: &str) -> String {
    let special_chars = ['_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!'];
    let mut result = String::with_capacity(text.len() * 2); // Предварительное выделение памяти
    
    for ch in text.chars() {
        if special_chars.contains(&ch) {
            result.push('\\');
        }
        result.push(ch);
    }
    
    result
}

pub async fn start_scheduler(bot: Bot, storage: Arc<JsonStorage>, weather_client: WeatherClient) {
    info!("Планировщик уведомлений запущен. Проверка расписания будет выполняться каждую минуту");
    
    loop {
        // Удаляем webhook в начале каждого цикла для предотвращения конфликтов
        if let Err(e) = bot.delete_webhook().await {
            error!("Ошибка при удалении webhook в планировщике: {}", e);
        }
        
        let now = Local::now();
        let now_time = now.format("%H:%M").to_string();
        let today = now.weekday();
        
        info!("Проверка расписания уведомлений [{}]", now_time);
        
        // Получаем всех пользователей из хранилища
        let users = storage.get_all_users().await;
        info!("Всего пользователей в базе: {}", users.len());

        // Проверяем, не настало ли время для массовой рассылки (12:00 или 18:00)
        let hours = now.hour();
        let minutes = now.minute();
        let is_mass_notification_time = (hours == 12 || hours == 18) && minutes == 0;
        
        info!("Текущее время: {}, массовая рассылка: {}", now_time, is_mass_notification_time);
        
        if is_mass_notification_time {
            info!("Время массовой рассылки [{}]. Отправляем уведомления всем пользователям.", now_time);
            
            // Дополнительно удаляем webhook перед массовой рассылкой
            if let Err(e) = bot.delete_webhook().await {
                error!("Ошибка при удалении webhook перед массовой рассылкой: {}", e);
            } else {
                info!("Webhook успешно удален перед массовой рассылкой");
            }
            
            send_mass_notifications(&bot, &users, &weather_client, &now_time, today).await;
            
            // Снова удаляем webhook после массовой рассылки
            if let Err(e) = bot.delete_webhook().await {
                error!("Ошибка при удалении webhook после массовой рассылки: {}", e);
            } else {
                info!("Webhook успешно удален после массовой рассылки");
            }
        }

        // Обычная проверка индивидуальных уведомлений
        for user in users {
            if let Some(scheduled_time) = &user.notification_time {
                if scheduled_time == &now_time {
                    if let Some(city) = &user.city {
                        info!("Отправка уведомления пользователю ID: {}, город: {}", user.user_id, city);
                        
                        // Получаем погоду
                        match weather_client.get_weather(city).await {
                            Ok(weather_text) => {
                                // Формируем сообщение в зависимости от режима бота
                                let message = if user.cute_mode {
                                    // Милый режим: с приветствием и милыми сообщениями
                                    // Получаем приветствие и дополнительные сообщения
                                    let greeting = get_greeting(today);
                                    let cute_message = get_cute_message();
                                    let good_day_wish = get_good_day_wish();
                                    
                                    // Формируем полное сообщение с экранированием
                                    format!("{}\n\n🌦 *Погода в {}*\n\n{}\n\n{}\n\n{}", 
                                        escape_markdown_v2(&greeting), 
                                        escape_markdown_v2(city), 
                                        escape_markdown_v2(&weather_text), 
                                        escape_markdown_v2(&cute_message), 
                                        escape_markdown_v2(&good_day_wish))
                                } else {
                                    // Стандартный режим: только погода
                                    format!("🌅 *Утренний прогноз погоды*\n\n🌦 *Погода в {}*\n\n{}", 
                                        escape_markdown_v2(city), 
                                        escape_markdown_v2(&weather_text))
                                };
                                
                                // Отправляем сообщение
                                if let Err(e) = bot.send_message(ChatId(user.user_id), message)
                                    .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                                    .await 
                                {
                                    error!("Не удалось отправить уведомление пользователю {}: {}", user.user_id, e);
                                } else {
                                    info!("Уведомление успешно отправлено пользователю ID: {}", user.user_id);
                                }
                            }
                            Err(e) => {
                                warn!("Ошибка получения погоды для пользователя {}: {}", user.user_id, e);
                                
                                // Отправляем уведомление об ошибке
                                let error_message = if user.cute_mode {
                                    format!("Доброе утро\\! К сожалению, не удалось получить данные о погоде: {}", 
                                        escape_markdown_v2(&e.to_string()))
                                } else {
                                    format!("❌ *Ошибка*: Не удалось получить данные о погоде: {}", 
                                        escape_markdown_v2(&e.to_string()))
                                };
                                
                                if let Err(e) = bot.send_message(
                                    ChatId(user.user_id),
                                    error_message
                                ).parse_mode(teloxide::types::ParseMode::MarkdownV2).await {
                                    error!("Не удалось отправить уведомление об ошибке пользователю {}: {}", user.user_id, e);
                                }
                            }
                        }
                    } else {
                        warn!("У пользователя ID: {} не установлен город", user.user_id);
                    }
                }
            }
        }
        
        // Ждем минуту перед следующей проверкой
        info!("Следующая проверка расписания через 1 минуту");
        sleep(Duration::from_secs(60)).await;
    }
}

// Приветствие с учетом дня недели
fn get_greeting(day: Weekday) -> String {
    match day {
        Weekday::Mon => "*Доброе утро, милая!* ✨\nНачинается новая неделя, и я знаю, что ты справишься со всем!".to_string(),
        Weekday::Tue => "*Доброе утречко!* 🌸\nУже вторник! День, когда можно горы свернуть!".to_string(),
        Weekday::Wed => "*Доброе утро, солнышко!* 💫\nСередина недели - время для маленьких радостей!".to_string(),
        Weekday::Thu => "*Доброе утро, красотка!* 🌿\nЧетверг - почти пятница! Ты молодец!".to_string(),
        Weekday::Fri => "*С добрым утром!* 🎉\nПятница наступила! Впереди выходные!".to_string(),
        Weekday::Sat => "*Доброе утро!* ☀️\nНаконец-то суббота! Время для отдыха и приятных дел!".to_string(),
        Weekday::Sun => "*Доброе утречко!* 🌤️\nВоскресенье - идеальный день, чтобы побаловать себя!".to_string(),
    }
}

// Генерация милого сообщения
fn get_cute_message() -> String {
    let messages = [
        "Ты самая прекрасная! Не забывай улыбаться сегодня! 💕",
        "Твоя улыбка способна осветить даже самый пасмурный день! 💖",
        "Не позволяй никому испортить твое настроение сегодня! Ты заслуживаешь только счастья! ✨",
        "Сегодня отличный день, чтобы начать что-то новое! Я верю в тебя! 🌟",
        "Помни, что ты особенная и удивительная! 💫",
        "Даже в самый обычный день важно находить моменты счастья! 🌸",
        "Твоя энергия и позитив заряжают всех вокруг! Так держать! 💝",
        "Надеюсь, сегодня тебя ждут приятные сюрпризы! 🎁",
        "Пусть этот день принесет тебе много радости и успехов! 🌈",
        "Ты сильнее, чем думаешь! Сегодня день новых возможностей! ⭐",
    ];
    
    let index = rand::thread_rng().gen_range(0..messages.len());
    messages[index].to_string()
}

// Пожелание хорошего дня
fn get_good_day_wish() -> String {
    let wishes = [
        "Желаю тебе чудесного дня! 💫",
        "Пусть сегодня тебя окружает только позитив! 🌈",
        "Хорошего и продуктивного дня! ✨",
        "Желаю, чтобы этот день был наполнен приятными моментами! 💖",
        "Пусть твой день будет таким же прекрасным, как и ты! 🌸",
        "Верю, что сегодня у тебя всё получится! 💪",
        "Удачного дня и легкого настроения! 🍀",
        "Пусть каждый час этого дня подарит тебе что-то хорошее! ⏰",
        "Прекрасного настроения на весь день! 🌞",
        "Пусть сегодня всё идет по твоему плану! 📝"
    ];
    
    let index = rand::thread_rng().gen_range(0..wishes.len());
    wishes[index].to_string()
}

// Функция для отправки уведомлений всем пользователям
async fn send_mass_notifications(
    bot: &Bot, 
    users: &Vec<super::storage::UserSettings>, 
    weather_client: &WeatherClient,
    time: &str,
    day: Weekday
) {
    for user in users {
        if let Some(city) = &user.city {
            info!("Отправка массового уведомления пользователю ID: {}, город: {}", user.user_id, city);
            
            // Получаем погоду
            match weather_client.get_weather(city).await {
                Ok(weather_text) => {
                    // Получаем сообщение в соответствии с режимом пользователя
                    let message = if user.cute_mode {
                        // Милый режим: приветствие и милые сообщения
                        let greeting = if time == "12:00" {
                            get_noon_greeting(day)
                        } else {
                            get_evening_greeting(day)
                        };
                        
                        // Получаем милое сообщение
                        let cute_message = get_cute_message();
                        
                        // Формируем полное сообщение с экранированием
                        format!("{}\n\n🌦 *Погода в {}*\n\n{}\n\n{}", 
                            escape_markdown_v2(&greeting), 
                            escape_markdown_v2(city), 
                            escape_markdown_v2(&weather_text), 
                            escape_markdown_v2(&cute_message))
                    } else {
                        // Стандартный режим: только погода
                        let greeting = if time == "12:00" {
                            "🕛 *Дневной прогноз погоды*".to_string()
                        } else {
                            "🌆 *Вечерний прогноз погоды*".to_string()
                        };
                        
                        format!("{}\n\n🌦 *Погода в {}*\n\n{}", 
                            greeting, 
                            escape_markdown_v2(city), 
                            escape_markdown_v2(&weather_text))
                    };
                    
                    // Отправляем сообщение
                    if let Err(e) = bot.send_message(ChatId(user.user_id), message)
                        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                        .await 
                    {
                        error!("Не удалось отправить массовое уведомление пользователю {}: {}", user.user_id, e);
                    } else {
                        info!("Массовое уведомление успешно отправлено пользователю ID: {}", user.user_id);
                    }
                }
                Err(e) => {
                    warn!("Ошибка получения погоды для пользователя {}: {}", user.user_id, e);
                }
            }
        }
    }
}

// Дневные приветствия
fn get_noon_greeting(day: Weekday) -> String {
    match day {
        Weekday::Mon => "*Добрый день!* 🌤️\nНадеюсь, первая половина понедельника прошла продуктивно!".to_string(),
        Weekday::Tue => "*Добрый день!* ☀️\nВторник в самом разгаре! Как проходит твой день?".to_string(),
        Weekday::Wed => "*Добрый день!* 🌈\nСередина недели - время для небольшого перерыва и вкусного обеда!".to_string(),
        Weekday::Thu => "*Приятного дня!* 🌻\nЧетверг - почти пятница! Держись, осталось совсем немного!".to_string(),
        Weekday::Fri => "*Добрый день!* 🎉\nПятница, день прекрасный! Скоро выходные!".to_string(),
        Weekday::Sat => "*Прекрасного дня!* 🍹\nНадеюсь, твоя суббота наполнена приятными моментами!".to_string(),
        Weekday::Sun => "*Добрый день!* 🌞\nВоскресенье - время отдыха и подготовки к новой неделе!".to_string(),
    }
}

// Вечерние приветствия
fn get_evening_greeting(day: Weekday) -> String {
    match day {
        Weekday::Mon => "*Добрый вечер!* 🌙\nПервый день недели почти позади! Ты молодец!".to_string(),
        Weekday::Tue => "*Добрый вечер!* 🌆\nКак прошел твой вторник? Надеюсь, продуктивно и с улыбкой!".to_string(),
        Weekday::Wed => "*Добрый вечер!* ✨\nСередина недели позади! Ты уже на пути к выходным!".to_string(),
        Weekday::Thu => "*Приятного вечера!* 🌟\nЗавтра пятница! Совсем немного осталось!".to_string(),
        Weekday::Fri => "*Прекрасного вечера!* 🥂\nПоздравляю с началом выходных! Пора отдохнуть!".to_string(),
        Weekday::Sat => "*Добрый вечер!* 🎭\nНадеюсь, суббота была наполнена приятными событиями!".to_string(),
        Weekday::Sun => "*Спокойного вечера!* 🌠\nВпереди новая неделя! Время настроиться на продуктивный лад!".to_string(),
    }
}