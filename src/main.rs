use crate::storage::{JsonStorage, UserSettings};
use dotenv::dotenv;
use std::sync::Arc;
use teloxide::prelude::*;
use log::{info, error};
use teloxide::utils::command::BotCommands;

mod weather;
mod storage;
mod scheduler;

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "Доступные команды:")]
enum Command {
    #[command(description = "начать работу с ботом")]
    Start,
    #[command(description = "показать это сообщение")]
    Help,
    #[command(description = "установить город (например, /city Москва)")]
    City(String),
    #[command(description = "установить время уведомлений (например, /time 08:00)")]
    Time(String),
    #[command(description = "узнать текущую погоду")]
    Weather,
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    // Устанавливаем уровень логирования на info, если не задан
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }
    pretty_env_logger::init();
    info!("Запуск FerrisBot...");

    let bot_token = std::env::var("TELEGRAM_BOT_TOKEN").expect("TELEGRAM_BOT_TOKEN не задан в .env файле");
    let weather_api_key = std::env::var("OPENWEATHER_API_KEY").expect("OPENWEATHER_API_KEY не задан в .env файле");

    // Создаем главный Arc
    let storage = Arc::new(JsonStorage::new("users.json").await);

    // Создаем клоны для разных задач
    let storage_for_handler = Arc::clone(&storage); 
    let storage_for_scheduler = Arc::clone(&storage);

    let bot = Bot::new(bot_token);
    let weather_client = weather::WeatherClient::new(weather_api_key.clone());
    
    // Принудительно устанавливаем команды в меню бота и проверяем результат
    info!("Настраиваю командную панель бота...");

    // Создаем список команд вручную для гарантированной поддержки
    use teloxide::types::BotCommand;

    let commands = vec![
        BotCommand::new("start", "начать работу с ботом"),
        BotCommand::new("help", "показать список команд"),
        BotCommand::new("city", "установить город (например, /city Москва)"),
        BotCommand::new("time", "установить время уведомлений (например, /time 08:00)"),
        BotCommand::new("weather", "узнать текущую погоду"),
    ];
    
    // Устанавливаем команды для всех чатов
    match bot.set_my_commands(commands).await {
        Ok(_) => info!("Командная панель бота успешно обновлена"),
        Err(e) => error!("Не удалось установить команды бота: {}", e),
    }

    // Настраиваем обработчик команд
    let command_handler = Update::filter_message()
        .branch(
            dptree::entry()
                .filter_command::<Command>()
                .endpoint(handle_commands),
        )
        .branch(dptree::endpoint(handle_message));

    // Планировщик уведомлений
    let scheduler_task = scheduler::start_scheduler(
        bot.clone(),
        storage_for_scheduler,
        weather_client.clone()
    );
    info!("Планировщик уведомлений запущен");

    // Указываем зависимости для обработчика
    let handler = dptree::deps![bot.clone(), storage_for_handler, weather_client];

    // Запускаем обе задачи параллельно
    let mut dispatcher = teloxide::dispatching::Dispatcher::builder(bot, command_handler)
        .dependencies(handler)
        .enable_ctrlc_handler()
        .build();
        
    let bot_task = dispatcher.dispatch();

    info!("Бот готов к работе!");
    tokio::select! {
        _ = bot_task => {
            info!("Бот остановлен");
        }
        _ = scheduler_task => {
            error!("Планировщик уведомлений остановлен неожиданно");
        }
    }
}

async fn handle_commands(
    bot: Bot,
    msg: Message,
    cmd: Command,
    storage: Arc<JsonStorage>,
    weather_client: weather::WeatherClient,
) -> ResponseResult<()> {
    let user_id = msg.chat.id.0;
    let username = msg.from()
        .and_then(|user| user.username.clone())
        .unwrap_or_else(|| format!("ID: {}", user_id));
    
    // Логируем полученную команду
    match &cmd {
        Command::Start => info!("Пользователь @{} запустил бота", username),
        Command::Help => info!("Пользователь @{} запросил помощь", username),
        Command::City(city) => info!("Пользователь @{} устанавливает город: {}", username, city),
        Command::Time(time) => info!("Пользователь @{} устанавливает время уведомлений: {}", username, time),
        Command::Weather => info!("Пользователь @{} запрашивает погоду", username),
    }
    
    match cmd {
        Command::Start => {
            send_start_message(&bot, &msg).await?;
        }
        Command::Help => {
            send_help(&bot, &msg).await?;
        }
        Command::City(city) => {
            set_city(&bot, &msg, &storage, &city).await?;
        }
        Command::Time(time) => {
            set_time(&bot, &msg, &storage, &time).await?;
        }
        Command::Weather => {
            send_current_weather(&bot, &msg, &storage, &weather_client).await?;
        }
    }
    Ok(())
}

async fn handle_message(bot: Bot, msg: Message) -> ResponseResult<()> {
    if let Some(text) = msg.text() {
        // Логируем текстовые сообщения
        let user_id = msg.chat.id.0;
        let username = msg.from()
            .and_then(|user| user.username.clone())
            .unwrap_or_else(|| format!("ID: {}", user_id));
        
        info!("Пользователь @{} отправил сообщение: {}", username, text);
        
        bot.send_message(
            msg.chat.id, 
            "Я понимаю только команды. Используйте /help для получения списка доступных команд."
        ).await?;
    }
    Ok(())
}

async fn send_start_message(bot: &Bot, msg: &Message) -> ResponseResult<()> {
    let text = "🌸 *Добро пожаловать в FerrisBot!*\n\n\
                Я твой персональный утренний бот-будильник с погодой и милыми сообщениями! \
                Каждое утро я буду отправлять тебе актуальный прогноз погоды и поднимать настроение.\n\n\
                *Что я умею:*\n\
                • 🌦️ Отправлять ежедневный подробный прогноз погоды в твоем городе\n\
                • 💌 Добавлять милые послания и пожелания хорошего дня\n\
                • 🕒 Выводить прогноз по запросу в любое время\n\n\
                *Настройки:*\n\
                /city [город] - установить твой город (например: /city Москва)\n\
                /time [HH:MM] - установить время ежедневных уведомлений (например: /time 08:00)\n\
                /weather - получить текущий прогноз погоды\n\
                /help - показать список всех команд\n\n\
                Пожалуйста, начни с установки города командой /city 💖";

    bot.send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .await?;
    Ok(())
}

async fn send_help(bot: &Bot, msg: &Message) -> ResponseResult<()> {
    let help_text = "🌟 *Доступные команды:*\n\n\
                     /start - начать работу с ботом\n\
                     /help - показать это сообщение\n\
                     /city [название] - установить город (например: /city Москва)\n\
                     /time [ЧЧ:ММ] - установить время уведомлений (например: /time 08:00)\n\
                     /weather - узнать текущую погоду";

    bot.send_message(msg.chat.id, help_text)
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .await?;
    Ok(())
}

async fn set_city(bot: &Bot, msg: &Message, storage: &JsonStorage, city_arg: &str) -> ResponseResult<()> {
    let user_id = msg.chat.id.0;
    let username = msg.from()
        .and_then(|user| user.username.clone())
        .unwrap_or_else(|| format!("ID: {}", user_id));
    
    // Проверка что город не пустой
    if city_arg.trim().is_empty() {
        info!("Пользователь @{} пытался установить пустой город", username);
        bot.send_message(
            msg.chat.id, 
            "🚫 Пожалуйста, укажите город после команды. Например: /city Москва"
        ).await?;
        return Ok(());
    }

    let mut user = storage.get_user(user_id).await.unwrap_or_else(|| UserSettings {
        user_id,
        city: None,
        notification_time: None,
    });

    user.city = Some(city_arg.trim().to_string());
    storage.save_user(user).await;
    
    info!("Пользователь @{} успешно установил город: {}", username, city_arg.trim());

    bot.send_message(
        msg.chat.id, 
        format!("🌆 *Город успешно установлен:* {}\n\nТеперь ты можешь:\n• Узнать текущую погоду с помощью /weather\n• Установить время для ежедневных уведомлений командой /time [HH:MM]", city_arg.trim())
    )
    .parse_mode(teloxide::types::ParseMode::MarkdownV2)
    .await?;
    
    Ok(())
}

async fn set_time(bot: &Bot, msg: &Message, storage: &JsonStorage, time_arg: &str) -> ResponseResult<()> {
    let user_id = msg.chat.id.0;
    let username = msg.from()
        .and_then(|user| user.username.clone())
        .unwrap_or_else(|| format!("ID: {}", user_id));
    
    // Проверка корректности формата времени
    if time_arg.trim().is_empty() {
        info!("Пользователь @{} пытался установить пустое время", username);
        bot.send_message(
            msg.chat.id, 
            "🚫 Пожалуйста, укажите время в формате HH:MM. Например: /time 08:00"
        ).await?;
        return Ok(());
    }
    
    // Проверяем формат времени (HH:MM)
    if !is_valid_time_format(time_arg.trim()) {
        info!("Пользователь @{} указал некорректный формат времени: {}", username, time_arg);
        bot.send_message(
            msg.chat.id, 
            "⚠️ Некорректный формат времени. Используйте формат HH:MM, например: 08:00"
        ).await?;
        return Ok(());
    }

    let mut user = storage.get_user(user_id).await.unwrap_or_else(|| UserSettings {
        user_id,
        city: None,
        notification_time: None,
    });

    user.notification_time = Some(time_arg.trim().to_string());
    storage.save_user(user).await;
    
    info!("Пользователь @{} успешно установил время уведомлений: {}", username, time_arg.trim());

    bot.send_message(
        msg.chat.id, 
        format!("⏰ *Время уведомлений установлено:* {}\n\nТеперь каждый день в это время я буду отправлять тебе прогноз погоды и милое сообщение! 💖", time_arg.trim())
    )
    .parse_mode(teloxide::types::ParseMode::MarkdownV2)
    .await?;
    
    Ok(())
}

async fn send_current_weather(
    bot: &Bot, 
    msg: &Message, 
    storage: &JsonStorage, 
    weather_client: &weather::WeatherClient
) -> ResponseResult<()> {
    let user_id = msg.chat.id.0;
    let username = msg.from()
        .and_then(|user| user.username.clone())
        .unwrap_or_else(|| format!("ID: {}", user_id));
    
    // Получаем настройки пользователя
    let user = storage.get_user(user_id).await;
    
    match user {
        Some(user) => {
            match &user.city {
                Some(city) => {
                    bot.send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing).await?;
                    
                    info!("Запрашиваю погоду для пользователя @{}, город: {}", username, city);
                    
                    match weather_client.get_weather(city).await {
                        Ok(weather) => {
                            info!("Успешно получена погода для пользователя @{}", username);
                            bot.send_message(msg.chat.id, format!("🌦️ *Погода в {}*\n\n{}", city, weather))
                                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                                .await?;
                        }
                        Err(e) => {
                            error!("Ошибка получения погоды для пользователя @{}: {}", username, e);
                            bot.send_message(
                                msg.chat.id, 
                                format!("❌ *Не удалось получить погоду:*\n{}\n\nПроверь правильность названия города или попробуй позже.", e)
                            )
                            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                            .await?;
                        }
                    }
                }
                None => {
                    info!("Пользователь @{} запросил погоду без установленного города", username);
                    bot.send_message(
                        msg.chat.id, 
                        "⚠️ *Город не установлен*\n\nПожалуйста, используй команду /city [город], чтобы я мог показать тебе прогноз погоды."
                    )
                    .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                    .await?;
                }
            }
        }
        None => {
            info!("Пользователь @{} запросил погоду без настройки профиля", username);
            bot.send_message(
                msg.chat.id, 
                "⚠️ *Требуется настройка*\n\nПожалуйста, настрой бота с помощью команды /city [город]."
            )
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
            .await?;
        }
    }
    
    Ok(())
}

fn is_valid_time_format(time: &str) -> bool {
    if let Some((hours_str, minutes_str)) = time.split_once(':') {
        if let (Ok(hours), Ok(minutes)) = (hours_str.parse::<u8>(), minutes_str.parse::<u8>()) {
            return hours < 24 && minutes < 60;
        }
    }
    false
}
