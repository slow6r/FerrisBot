use teloxide::types::ChatId;
use teloxide::Bot;
use super::storage::JsonStorage;
use super::weather::WeatherClient;
use chrono::{Local, Datelike, Weekday};
use tokio::time::{sleep, Duration};
use std::sync::Arc;
use teloxide::payloads::SendMessageSetters;
use teloxide::prelude::Requester;
use rand::Rng;
use log::{info, error, warn};

pub async fn start_scheduler(bot: Bot, storage: Arc<JsonStorage>, weather_client: WeatherClient) {
    info!("Планировщик уведомлений запущен. Проверка расписания будет выполняться каждый час");
    
    loop {
        let now = Local::now();
        let now_time = now.format("%H:%M").to_string();
        let today = now.weekday();
        
        info!("Проверка расписания уведомлений [{}]", now_time);
        
        // Получаем всех пользователей из хранилища
        let users = storage.get_all_users().await;
        info!("Всего пользователей в базе: {}", users.len());

        // Проверяем, не настало ли время для массовой рассылки (12:00 или 18:00)
        let is_mass_notification_time = now_time == "12:00" || now_time == "18:00";
        
        if is_mass_notification_time {
            info!("Время массовой рассылки [{}]. Отправляем уведомления всем пользователям.", now_time);
            send_mass_notifications(&bot, &users, &weather_client, &now_time, today).await;
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
                                // Получаем приветствие и дополнительные сообщения
                                let greeting = get_greeting(today);
                                let cute_message = get_cute_message();
                                let good_day_wish = get_good_day_wish();
                                
                                // Формируем полное сообщение
                                let message = format!("{}\n\n🌦 *Погода в {}*\n\n{}\n\n{}\n\n{}", 
                                    greeting, city, weather_text, cute_message, good_day_wish);
                                
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
                                if let Err(e) = bot.send_message(
                                    ChatId(user.user_id),
                                    format!("Доброе утро! К сожалению, не удалось получить данные о погоде: {}", e)
                                ).await {
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
        
        // Ждем час перед следующей проверкой
        info!("Следующая проверка расписания через 1 час");
        sleep(Duration::from_secs(3600)).await;
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
    let greeting = if time == "12:00" {
        get_noon_greeting(day)
    } else {
        get_evening_greeting(day)
    };

    for user in users {
        if let Some(city) = &user.city {
            info!("Отправка массового уведомления пользователю ID: {}, город: {}", user.user_id, city);
            
            // Получаем погоду
            match weather_client.get_weather(city).await {
                Ok(weather_text) => {
                    // Получаем милое сообщение
                    let cute_message = get_cute_message();
                    
                    // Формируем полное сообщение
                    let message = format!("{}\n\n🌦 *Погода в {}*\n\n{}\n\n{}", 
                        greeting, city, weather_text, cute_message);
                    
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