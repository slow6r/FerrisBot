use crate::storage::{JsonStorage, UserSettings};
use dotenv::dotenv;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use log::{info, error};
use teloxide::utils::command::BotCommands;

mod weather;
mod storage;
mod scheduler;
mod utils;

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
    let storage_for_callback = Arc::clone(&storage);

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
        BotCommand::new("city", "установить город"),
        BotCommand::new("time", "установить время уведомлений"),
        BotCommand::new("weather", "узнать текущую погоду"),
        BotCommand::new("forecast", "получить прогноз погоды на неделю"),
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
        
    // Добавляем обработчик для колбэков от кнопок
    let callback_handler = Update::filter_callback_query()
        .branch(dptree::endpoint(handle_callback_query));
        
    // Настраиваем маршрутизацию
    let handler = dptree::entry()
        .branch(command_handler)
        .branch(callback_handler);

    // Планировщик уведомлений
    let scheduler_task = scheduler::start_scheduler(
        bot.clone(),
        storage_for_scheduler,
        weather_client.clone()
    );
    info!("Планировщик уведомлений запущен");

    // Указываем зависимости для обработчика
    let dependencies = dptree::deps![bot.clone(), storage_for_handler, storage_for_callback, weather_client];

    // Запускаем обе задачи параллельно
    let mut dispatcher = teloxide::dispatching::Dispatcher::builder(bot, handler)
        .dependencies(dependencies)
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
        )
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .await?;
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
    let standard_text = "📱 *Добро пожаловать в FerrisBot!*\n\n\
                Я твой персональный бот\\-помощник с погодой\\! \
                Каждое утро я буду отправлять тебе актуальный прогноз погоды в указанное время\\.\n\n\
                *Что я умею:*\n\
                • 🌦️ Отправлять ежедневный прогноз погоды в твоем городе\n\
                • 🕒 Автоматически присылать прогноз в указанное время\n\
                • 🔍 Предоставлять прогноз по запросу в любое время\n\n\
                *Настройки:*\n\
                /city \\- выбрать город из списка или ввести свой\n\
                /time \\- выбрать время уведомлений из списка или ввести своё\n\
                /weather \\- получить текущий прогноз погоды\n\
                /forecast \\- получить прогноз погоды на неделю\n\
                /help \\- показать список всех команд\n\n\
                *Для начала работы* нажмите /city для выбора города\\!";

    // Создаем кнопку для быстрого перехода к выбору города
    let keyboard = InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            "🏙️ Выбрать город".to_string(),
            "choose_city".to_string(),
        )],
    ]);

    bot.send_message(msg.chat.id, standard_text)
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .reply_markup(keyboard)
        .await?;
    Ok(())
}

// Функция для отображения меню выбора города
async fn show_city_selection(bot: &Bot, chat_id: ChatId) -> ResponseResult<()> {
    // Список популярных городов
    let cities = vec![
        "Москва", "Санкт-Петербург", "Новосибирск", "Екатеринбург", 
        "Казань", "Нижний Новгород", "Челябинск", "Самара", "Омск", "Ростов-на-Дону"
    ];
    
    // Создаем кнопки с городами
    let mut keyboard = Vec::new();
    for chunk in cities.chunks(2) {
        let row = chunk.iter()
            .map(|city| InlineKeyboardButton::callback(city.to_string(), format!("set_city:{}", city)))
            .collect::<Vec<_>>();
        keyboard.push(row);
    }
    
    // Добавляем кнопку для ручного ввода
    keyboard.push(vec![InlineKeyboardButton::callback(
        "🔎 Ввести другой город...".to_string(),
        "manual_city".to_string(),
    )]);
    
    let keyboard = InlineKeyboardMarkup::new(keyboard);
    
    bot.send_message(
        chat_id,
        "Выберите город из списка или нажмите *Ввести другой город* для ручного ввода\\."
    )
    .parse_mode(teloxide::types::ParseMode::MarkdownV2)
    .reply_markup(keyboard)
    .await?;
    
    Ok(())
}

// Функция для отображения меню выбора времени
async fn show_time_selection(bot: &Bot, chat_id: ChatId) -> ResponseResult<()> {
    // Список популярных вариантов времени
    let times = vec![
        "07:00", "07:30", "08:00", "08:30", "09:00", 
        "09:30", "10:00", "10:30", "11:00", "12:00"
    ];
    
    // Создаем кнопки с временем
    let mut keyboard = Vec::new();
    for chunk in times.chunks(5) {
        let row = chunk.iter()
            .map(|time| InlineKeyboardButton::callback(time.to_string(), format!("set_time:{}", time)))
            .collect::<Vec<_>>();
        keyboard.push(row);
    }
    
    // Добавляем кнопку для ручного ввода
    keyboard.push(vec![InlineKeyboardButton::callback(
        "⌨️ Ввести другое время...".to_string(),
        "manual_time".to_string(),
    )]);
    
    let keyboard = InlineKeyboardMarkup::new(keyboard);
    
    bot.send_message(
        chat_id,
        "Выберите время для ежедневных уведомлений о погоде из списка или нажмите *Ввести другое время* для ручного ввода в формате HH:MM\\."
    )
    .parse_mode(teloxide::types::ParseMode::MarkdownV2)
    .reply_markup(keyboard)
    .await?;
    
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
         /city \\- выбрать город из списка или ввести свой\n\
         /time \\- выбрать время ежедневных уведомлений из списка или ввести своё\n\
         /weather \\- узнать текущую погоду\n\
         /forecast \\- получить прогноз погоды на неделю 💖"
    } else {
        "🌟 *Доступные команды:*\n\n\
         /start \\- начать работу с ботом\n\
         /help \\- показать это сообщение\n\
         /city \\- выбрать город из списка или ввести свой\n\
         /time \\- выбрать время уведомлений из списка или ввести своё\n\
         /weather \\- узнать текущую погоду\n\
         /forecast \\- получить прогноз погоды на неделю"
    };

    // Добавляем кнопки для быстрого доступа
    let keyboard = InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            "🏙️ Выбрать город".to_string(),
            "choose_city".to_string(),
        )],
        vec![InlineKeyboardButton::callback(
            "⏰ Выбрать время".to_string(),
            "choose_time".to_string(),
        )],
    ]);

    bot.send_message(msg.chat.id, help_text)
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .reply_markup(keyboard)
        .await?;
    Ok(())
}

async fn set_city(bot: &Bot, msg: &Message, storage: &JsonStorage, city_arg: &str) -> ResponseResult<()> {
    let user_id = msg.chat.id.0;
    let username = msg.from()
        .and_then(|user| user.username.clone())
        .unwrap_or_else(|| format!("ID: {}", user_id));
    
    // Если аргумент пустой, показываем меню выбора города
    if city_arg.trim().is_empty() {
        info!("Пользователь @{} запросил меню выбора города", username);
        return show_city_selection(bot, msg.chat.id).await;
    }

    // Проверка что город не пустой (для обратной совместимости)
    let city_name = city_arg.trim();
    info!("Пользователь @{} устанавливает город: {}", username, city_name);

    let mut user = storage.get_user(user_id).await.unwrap_or_else(|| UserSettings {
        user_id,
        city: None,
        notification_time: None,
        cute_mode: false, // По умолчанию стандартный режим
    });

    // Сохраняем флаг cute_mode перед сохранением пользователя
    let is_cute_mode = user.cute_mode;
    
    user.city = Some(city_name.to_string());
    storage.save_user(user).await;
    
    info!("Пользователь @{} успешно установил город: {}", username, city_name);

    // Формируем сообщение в зависимости от режима
    let message = if is_cute_mode {
        format!("🌆 *Город успешно установлен:* {}\n\nТеперь ты можешь:\n• Узнать текущую погоду с помощью /weather\n• Установить время для ежедневных уведомлений командой /time", utils::escape_markdown_v2(city_name))
    } else {
        format!("🌆 *Город успешно установлен:* {}\n\nВы можете:\n• Узнать текущую погоду с помощью /weather\n• Установить время для ежедневных уведомлений командой /time", utils::escape_markdown_v2(city_name))
    };

    // Создаем кнопку для быстрого перехода к выбору времени
    let keyboard = InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            "⏰ Выбрать время уведомлений".to_string(),
            "choose_time".to_string(),
        )],
    ]);

    bot.send_message(msg.chat.id, message)
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .reply_markup(keyboard)
        .await?;
    
    Ok(())
}

async fn set_time(bot: &Bot, msg: &Message, storage: &JsonStorage, time_arg: &str) -> ResponseResult<()> {
    let user_id = msg.chat.id.0;
    let username = msg.from()
        .and_then(|user| user.username.clone())
        .unwrap_or_else(|| format!("ID: {}", user_id));
    
    // Если аргумент пустой, показываем меню выбора времени
    if time_arg.trim().is_empty() {
        info!("Пользователь @{} запросил меню выбора времени", username);
        return show_time_selection(bot, msg.chat.id).await;
    }
    
    // Проверка корректности формата времени
    let time_str = time_arg.trim();
    
    // Проверяем формат времени (HH:MM)
    if !is_valid_time_format(time_str) {
        info!("Пользователь @{} указал некорректный формат времени: {}", username, time_str);
        bot.send_message(
            msg.chat.id, 
            "⚠️ Некорректный формат времени\\. Используйте формат HH:MM, например: 08:00"
        )
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .await?;
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
    
    user.notification_time = Some(time_str.to_string());
    storage.save_user(user).await;
    
    info!("Пользователь @{} успешно установил время уведомлений: {}", username, time_str);

    // Сообщение в зависимости от режима
    let message = if is_cute_mode {
        format!("⏰ *Время уведомлений установлено:* {}\n\nТеперь каждый день в это время я буду отправлять тебе прогноз погоды и милое сообщение\\! 💖", utils::escape_markdown_v2(time_str))
    } else {
        format!("⏰ *Время уведомлений установлено:* {}\n\nТеперь каждый день в это время вы будете получать актуальный прогноз погоды\\.", utils::escape_markdown_v2(time_str))
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
                                    utils::escape_markdown_v2(city), 
                                    utils::escape_markdown_v2(&weather))
                            } else {
                                // Стандартный режим
                                format!("🌦️ *Погода в {}*\n\n{}", 
                                    utils::escape_markdown_v2(city), 
                                    utils::escape_markdown_v2(&weather))
                            };
                            
                            bot.send_message(msg.chat.id, message)
                                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                                .await?;
                        }
                        Err(e) => {
                            error!("Ошибка получения погоды для пользователя @{}: {}", username, e);
                            bot.send_message(
                                msg.chat.id, 
                                format!("❌ *Не удалось получить погоду:*\n{}\n\nПроверь правильность названия города или попробуй позже\\.", utils::escape_markdown_v2(&e.to_string()))
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
                            let city_escaped = utils::escape_markdown_v2(city);
                            let forecast_escaped = utils::escape_markdown_v2(&forecast);
                            
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
                                format!("❌ *Не удалось получить прогноз:*\n{}\n\nПроверь правильность названия города или попробуй позже\\.", utils::escape_markdown_v2(&e.to_string()))
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

// Функция обработки колбэков от inline-кнопок
async fn handle_callback_query(
    bot: Bot,
    query: CallbackQuery,
    storage: Arc<JsonStorage>,
) -> ResponseResult<()> {
    // Проверяем, есть ли данные в колбэк-запросе
    if let Some(data) = &query.data {
        // Клонируем message, чтобы избежать partial move
        let message_opt = query.message.clone();
        
        let chat_id = if let Some(message) = &message_opt {
            message.chat.id
        } else {
            // Если нет сообщения, просто возвращаемся
            return Ok(());
        };
        
        let username = query.from.username.clone().unwrap_or_else(|| format!("ID: {}", query.from.id.0));
        
        info!("Пользователь @{} нажал на кнопку с callback: {}", username, data);
        
        // Обрабатываем различные типы колбэков
        if data == "choose_city" {
            // Показываем меню выбора города
            show_city_selection(&bot, chat_id).await?;
        } else if data == "choose_time" {
            // Показываем меню выбора времени
            show_time_selection(&bot, chat_id).await?;
        } else if data == "manual_city" {
            // Просим пользователя ввести город вручную
            bot.send_message(
                chat_id,
                "Пожалуйста, введите название города в формате:\\n\\n/city Название\\_города"
            )
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
            .await?;
        } else if data == "manual_time" {
            // Просим пользователя ввести время вручную
            bot.send_message(
                chat_id,
                "Пожалуйста, введите время для уведомлений в формате HH:MM, например:\\n\\n/time 08:00"
            )
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
            .await?;
        } else if data.starts_with("set_city:") {
            // Устанавливаем город из выбранного значения
            let city = data.trim_start_matches("set_city:");
            
            // Используем существующую команду set_city
            if let Some(message) = &message_opt {
                set_city(&bot, message, &storage, city).await?;
            }
        } else if data.starts_with("set_time:") {
            // Устанавливаем время из выбранного значения
            let time = data.trim_start_matches("set_time:");
            
            // Используем существующую команду set_time
            if let Some(message) = &message_opt {
                set_time(&bot, message, &storage, time).await?;
            }
        }
        
        // Отвечаем на колбэк, чтобы убрать индикатор загрузки
        bot.answer_callback_query(query.id).await?;
    }
    
    Ok(())
}
