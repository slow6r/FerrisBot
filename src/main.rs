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
    #[command(description = "прогноз погоды на неделю")]
    Forecast,
}

// Вспомогательная функция для экранирования специальных символов Markdown
fn escape_markdown_v2(text: &str) -> String {
    // Создаем новую строку с запасом для экранирующих символов
    let mut result = String::with_capacity(text.len() * 2);
    
    for ch in text.chars() {
        // Особая обработка для восклицательного знака - двойной escaping
        if ch == '!' {
            result.push_str("\\\\!");
        }
        // Специальные символы MarkdownV2, которые нужно экранировать
        else if ['_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.'].contains(&ch) {
            result.push('\\');
            result.push(ch);
        } 
        else {
            result.push(ch);
        }
    }
    
    result
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
    
    // Удаляем webhook перед запуском бота, чтобы избежать конфликта с getUpdates
    if let Err(e) = bot.delete_webhook().await {
        error!("Ошибка при удалении webhook: {}", e);
    } else {
        info!("Webhook успешно удален");
    }
    
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
        BotCommand::new("forecast", "прогноз погоды на неделю"),
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
        Command::Forecast => info!("Пользователь @{} запрашивает прогноз на неделю", username),
    }
    
    match cmd {
        Command::Start => {
            send_start_message(&bot, &msg, &storage).await?;
        }
        Command::Help => {
            send_help(&bot, &msg, &storage).await?;
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
        Command::Forecast => {
            send_weekly_forecast(&bot, &msg, &storage, &weather_client).await?;
        }
    }
    Ok(())
}

async fn handle_message(bot: Bot, msg: Message, storage: Arc<JsonStorage>) -> ResponseResult<()> {
    if let Some(text) = msg.text() {
        // Логируем текстовые сообщения
        let user_id = msg.chat.id.0;
        let username = msg.from()
            .and_then(|user| user.username.clone())
            .unwrap_or_else(|| format!("ID: {}", user_id));
        
        info!("Пользователь @{} отправил сообщение: {}", username, text);
        
        // Секретный код для активации "милого режима"
        // Используем необычную комбинацию символов, которую сложно угадать случайно
        if text.trim() == "<3cute<3" {
            // Получаем текущие настройки пользователя
            let mut user = storage.get_user(user_id).await.unwrap_or_else(|| UserSettings {
                user_id,
                city: None,
                notification_time: None,
                cute_mode: false,
            });
            
            // Включаем милый режим
            user.cute_mode = true;
            storage.save_user(user).await;
            
            bot.send_message(
                msg.chat.id, 
                "💕 *Милый режим активирован\\!*\n\nТеперь бот будет отправлять тебе милые сообщения и пожелания\\. Твой персональный бот\\-помощник всегда рядом\\!"
            )
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
            .await?;
            
            info!("Пользователь @{} активировал милый режим", username);
            return Ok(());
        }
        
        // Код для отключения "милого режима"
        if text.trim() == "/std" {
            // Получаем текущие настройки пользователя
            let mut user = storage.get_user(user_id).await.unwrap_or_else(|| UserSettings {
                user_id,
                city: None,
                notification_time: None,
                cute_mode: false,
            });
            
            // Отключаем милый режим, если он был включен
            if user.cute_mode {
                user.cute_mode = false;
                storage.save_user(user).await;
                
                bot.send_message(
                    msg.chat.id, 
                    "🔄 Стандартный режим активирован\\. Бот будет отправлять только информативные сообщения о погоде\\."
                )
                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                .await?;
                
                info!("Пользователь @{} переключился на стандартный режим", username);
                return Ok(());
            }
        }
        
        // Стандартный ответ на прочие сообщения
        bot.send_message(
            msg.chat.id, 
            "Я понимаю только команды\\. Используйте /help для получения списка доступных команд\\."
        ).await?;
    }
    Ok(())
}

async fn send_start_message(bot: &Bot, msg: &Message, storage: &JsonStorage) -> ResponseResult<()> {
    let user_id = msg.chat.id.0;
    
    // Получаем или создаем настройки пользователя
    let mut user = storage.get_user(user_id).await.unwrap_or_else(|| UserSettings {
        user_id,
        city: None,
        notification_time: None,
        cute_mode: false, // Стандартный режим по умолчанию
    });
    
    // Принудительно устанавливаем стандартный режим при команде /start
    if user.cute_mode {
        user.cute_mode = false;
        storage.save_user(user).await;
    }
    
    // Всегда отправляем стандартное сообщение при /start
    let standard_text = "📱 *Добро пожаловать в FerrisBot\\!*\n\n\
                Я твой персональный бот\\-помощник с погодой\\! \
                Каждое утро я буду отправлять тебе актуальный прогноз погоды в указанное время\\.\n\n\
                *Что я умею:*\n\
                • 🌦️ Отправлять ежедневный прогноз погоды в твоем городе\n\
                • 🕒 Автоматически присылать прогноз в указанное время\n\
                • 🔍 Предоставлять прогноз по запросу в любое время\n\n\
                *Для начала работы:*\n\
                1️⃣ Сначала установи свой город командой /city \\[город\\] \\(например: /city Москва\\)\n\
                2️⃣ Затем установи время ежедневных уведомлений: /time \\[HH:MM\\] \\(например: /time 08:00\\)\n\
                3️⃣ Готово\\! Бот будет присылать прогноз погоды по расписанию\n\n\
                *Другие команды:*\n\
                /weather \\- получить текущий прогноз погоды\n\
                /forecast \\- получить прогноз погоды на неделю\n\
                /help \\- показать список всех команд";

    // Отправляем приветственное сообщение
    bot.send_message(msg.chat.id, standard_text)
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .await?;
    
    // Отправляем дополнительное сообщение с подсказкой
    bot.send_message(
        msg.chat.id,
        "👉 Пожалуйста, начните с установки вашего города командой:\n/city Москва\n(замените Москва на ваш город)"
    ).await?;
    
    Ok(())
}

async fn send_help(bot: &Bot, msg: &Message, storage: &JsonStorage) -> ResponseResult<()> {
    let user_id = msg.chat.id.0;
    
    // Получаем настройки пользователя
    let user = storage.get_user(user_id).await;
    let cute_mode = user.map(|u| u.cute_mode).unwrap_or(false);
    
    // Текст справки в зависимости от режима
    let help_text = if cute_mode {
        "✨ *Доступные команды:*\n\n\
         /start \\- начать работу с ботом\n\
         /help \\- показать это сообщение\n\
         /city \\[название\\] \\- установить твой город \\(например: /city Москва\\)\n\
         /time \\[ЧЧ:ММ\\] \\- установить время ежедневных уведомлений \\(например: /time 08:00\\)\n\
         /weather \\- узнать текущую погоду\n\
         /forecast \\- получить прогноз погоды на неделю 💖"
    } else {
        "🌟 *Доступные команды:*\n\n\
         /start \\- начать работу с ботом\n\
         /help \\- показать это сообщение\n\
         /city \\[название\\] \\- установить город \\(например: /city Москва\\)\n\
         /time \\[ЧЧ:ММ\\] \\- установить время уведомлений \\(например: /time 08:00\\)\n\
         /weather \\- узнать текущую погоду\n\
         /forecast \\- получить прогноз погоды на неделю"
    };

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
            "🚫 Пожалуйста, укажите город после команды\\. Например: /city Москва"
        ).await?;
        return Ok(());
    }

    let mut user = storage.get_user(user_id).await.unwrap_or_else(|| UserSettings {
        user_id,
        city: None,
        notification_time: None,
        cute_mode: false, // По умолчанию стандартный режим
    });

    // Сохраняем флаг cute_mode перед сохранением пользователя
    let is_cute_mode = user.cute_mode;
    
    user.city = Some(city_arg.trim().to_string());
    storage.save_user(user).await;
    
    info!("Пользователь @{} успешно установил город: {}", username, city_arg.trim());

    // Формируем сообщение в зависимости от режима
    let message = if is_cute_mode {
        format!("🌆 *Город успешно установлен:* {}\n\nТеперь ты можешь:\n• Узнать текущую погоду с помощью /weather\n• Установить время для ежедневных уведомлений командой /time \\[HH:MM\\]", escape_markdown_v2(city_arg.trim()))
    } else {
        format!("🌆 *Город успешно установлен:* {}\n\nВы можете:\n• Узнать текущую погоду с помощью /weather\n• Установить время для ежедневных уведомлений командой /time \\[HH:MM\\]", escape_markdown_v2(city_arg.trim()))
    };

    bot.send_message(msg.chat.id, message)
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
            "🚫 Пожалуйста, укажите время в формате HH:MM\\. Например: /time 08:00"
        ).await?;
        return Ok(());
    }
    
    // Проверяем формат времени (HH:MM)
    if !is_valid_time_format(time_arg.trim()) {
        info!("Пользователь @{} указал некорректный формат времени: {}", username, time_arg);
        bot.send_message(
            msg.chat.id, 
            "⚠️ Некорректный формат времени\\. Используйте формат HH:MM, например: 08:00"
        ).await?;
        return Ok(());
    }

    let mut user = storage.get_user(user_id).await.unwrap_or_else(|| UserSettings {
        user_id,
        city: None,
        notification_time: None,
        cute_mode: false, // По умолчанию стандартный режим
    });

    // Сохраняем флаг cute_mode перед сохранением пользователя
    let is_cute_mode = user.cute_mode;
    
    user.notification_time = Some(time_arg.trim().to_string());
    storage.save_user(user).await;
    
    info!("Пользователь @{} успешно установил время уведомлений: {}", username, time_arg.trim());

    // Сообщение в зависимости от режима
    let message = if is_cute_mode {
        format!("⏰ *Время уведомлений установлено:* {}\n\nТеперь каждый день в это время я буду отправлять тебе прогноз погоды и милое сообщение\\! 💖", escape_markdown_v2(time_arg.trim()))
    } else {
        format!("⏰ *Время уведомлений установлено:* {}\n\nТеперь каждый день в это время вы будете получать актуальный прогноз погоды\\.", escape_markdown_v2(time_arg.trim()))
    };

    bot.send_message(msg.chat.id, message)
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
                            
                            // Формируем сообщение в зависимости от режима
                            let message = if user.cute_mode {
                                // Милый режим
                                format!("💖 *Специально для тебя, погода в {}*\n\n{}", 
                                    escape_markdown_v2(city), 
                                    escape_markdown_v2(&weather))
                            } else {
                                // Стандартный режим
                                format!("🌦️ *Погода в {}*\n\n{}", 
                                    escape_markdown_v2(city), 
                                    escape_markdown_v2(&weather))
                            };
                            
                            bot.send_message(msg.chat.id, message)
                                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                                .await?;
                        }
                        Err(e) => {
                            error!("Ошибка получения погоды для пользователя @{}: {}", username, e);
                            bot.send_message(
                                msg.chat.id, 
                                format!("❌ *Не удалось получить погоду:*\n{}\n\nПроверь правильность названия города или попробуй позже\\.", escape_markdown_v2(&e.to_string()))
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
                        "⚠️ *Город не установлен*\n\nПожалуйста, используй команду /city \\[город\\], чтобы я мог показать тебе прогноз погоды\\."
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
                "⚠️ *Требуется настройка*\n\nПожалуйста, настрой бота с помощью команды /city \\[город\\]\\."
            )
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
            .await?;
        }
    }
    
    Ok(())
}

async fn send_weekly_forecast(
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
                    
                    info!("Запрашиваю прогноз на неделю для пользователя @{}, город: {}", username, city);
                    
                    match weather_client.get_weekly_forecast(city).await {
                        Ok(forecast) => {
                            info!("Успешно получен прогноз на неделю для пользователя @{}", username);
                            
                            // Экранируем специальные символы для MarkdownV2
                            let city_escaped = escape_markdown_v2(city);
                            let forecast_escaped = escape_markdown_v2(&forecast);
                            
                            // Формируем сообщение в зависимости от режима
                            let message = if user.cute_mode {
                                // Милый режим
                                format!("✨ *Прогноз погоды на неделю в {}*\n\nСпециально для тебя я подготовил(а) детальный прогноз:\n\n{}", city_escaped, forecast_escaped)
                            } else {
                                // Стандартный режим
                                format!("🗓 *Прогноз погоды на неделю в {}*\n\n{}", city_escaped, forecast_escaped)
                            };
                            
                            bot.send_message(msg.chat.id, message)
                                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                                .await?;
                        }
                        Err(e) => {
                            error!("Ошибка получения прогноза на неделю для пользователя @{}: {}", username, e);
                            bot.send_message(
                                msg.chat.id, 
                                format!("❌ *Не удалось получить прогноз:*\n{}\n\nПроверь правильность названия города или попробуй позже\\.", escape_markdown_v2(&e.to_string()))
                            )
                            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                            .await?;
                        }
                    }
                }
                None => {
                    info!("Пользователь @{} запросил прогноз на неделю без установленного города", username);
                    bot.send_message(
                        msg.chat.id, 
                        "⚠️ *Город не установлен*\n\nПожалуйста, используй команду /city \\[город\\], чтобы я мог показать тебе прогноз погоды\\."
                    )
                    .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                    .await?;
                }
            }
        }
        None => {
            info!("Пользователь @{} запросил прогноз на неделю без настройки профиля", username);
            bot.send_message(
                msg.chat.id, 
                "⚠️ *Требуется настройка*\n\nПожалуйста, настрой бота с помощью команды /city \\[город\\]\\."
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
