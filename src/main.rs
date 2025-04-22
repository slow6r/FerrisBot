use crate::storage::{JsonStorage, UserSettings};
use dotenv::dotenv;
use std::sync::Arc;
use teloxide::prelude::*;
use log::{info, error};
use teloxide::utils::command::BotCommands;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use teloxide::types::CallbackQuery;

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
    if let Err(e) = bot.delete_webhook().send().await {
        error!("Ошибка при удалении webhook: {}", e);
    } else {
        info!("Webhook успешно удален");
    }
    
    // Дополнительная проверка, что webhook действительно удален
    match bot.get_webhook_info().send().await {
        Ok(info) => {
            if let Some(url) = info.url {
                if url.to_string().is_empty() {
                    info!("Webhook отключен успешно");
                } else {
                    error!("Webhook всё ещё активен: {}", url);
                    if let Err(e) = bot.delete_webhook().send().await {
                        error!("Повторная попытка удаления webhook завершилась ошибкой: {}", e);
                    }
                }
            } else {
                info!("Webhook отключен успешно");
            }
        },
        Err(e) => error!("Не удалось получить информацию о webhook: {}", e),
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
    
    // Добавляем обработчик для колбэков от инлайн-клавиатуры
    let callback_handler = Update::filter_callback_query()
        .branch(dptree::endpoint(handle_callback_query));
    
    // Объединяем обработчики
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
    let handler_dependencies = dptree::deps![bot.clone(), storage_for_handler, weather_client];

    // Запускаем обе задачи параллельно
    let mut dispatcher = teloxide::dispatching::Dispatcher::builder(bot, handler)
        .dependencies(handler_dependencies)
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
        
        // Получаем данные пользователя для проверки состояния
        let user = storage.get_user(user_id).await;
        
        // Проверяем состояние пользователя
        if let Some(user_data) = user {
            if let Some(state) = &user_data.state {
                if state == "waiting_for_time" {
                    // Пользователь в режиме ввода времени
                    let time_input = text.trim();
                    
                    // Проверяем формат введенного времени
                    if is_valid_time_format(time_input) {
                        // Время корректное, сохраняем
                        let mut updated_user = user_data.clone();
                        updated_user.notification_time = Some(time_input.to_string());
                        updated_user.state = None; // Сбрасываем состояние ожидания
                        storage.save_user(updated_user).await;
                        
                        let is_cute_mode = user_data.cute_mode;
                        
                        // Формируем сообщение об успешной установке времени
                        let message = if is_cute_mode {
                            format!("⏰ Время уведомлений установлено: {}\n\nТеперь каждый день в это время я буду отправлять тебе прогноз погоды и милое сообщение! 💖", escape_markdown_v2(time_input))
                        } else {
                            format!("⏰ Время уведомлений установлено: {}\n\nТеперь каждый день в это время вы будете получать актуальный прогноз погоды.", escape_markdown_v2(time_input))
                        };
                        
                        bot.send_message(msg.chat.id, message)
                            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                            .await?;
                        
                        info!("Пользователь @{} успешно установил время уведомлений: {}", username, time_input);
                        return Ok(());
                    } else {
                        // Некорректный формат времени
                        bot.send_message(
                            msg.chat.id, 
                            "⚠️ Некорректный формат времени\n\nПожалуйста, введите время в формате ЧЧ:ММ (например: 08:30).\n\nДопустимое время: от 00:00 до 23:59"
                        )
                        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                        .await?;
                        return Ok(());
                    }
                } else if state == "waiting_for_city" {
                    // Пользователь в режиме ввода города
                    let city_input = text.trim();
                    
                    // Проверяем, что ввод не пустой
                    if !city_input.is_empty() {
                        // Город введен, сохраняем
                        let mut updated_user = user_data.clone();
                        updated_user.city = Some(city_input.to_string());
                        updated_user.state = None; // Сбрасываем состояние ожидания
                        storage.save_user(updated_user).await;
                        
                        let is_cute_mode = user_data.cute_mode;
                        
                        // Формируем сообщение об успешной установке города
                        let message = if is_cute_mode {
                            format!("🌆 Город успешно установлен: {}\n\nТеперь ты можешь:\n• Узнать текущую погоду с помощью /weather\n• Установить время для ежедневных уведомлений командой /time", escape_markdown_v2(city_input))
                        } else {
                            format!("🌆 Город успешно установлен: {}\n\nВы можете:\n• Узнать текущую погоду с помощью /weather\n• Установить время для ежедневных уведомлений командой /time", escape_markdown_v2(city_input))
                        };
                        
                        bot.send_message(msg.chat.id, message)
                            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                            .await?;
                        
                        info!("Пользователь @{} успешно установил город: {}", username, city_input);
                        return Ok(());
                    } else {
                        // Пустой ввод города
                        bot.send_message(
                            msg.chat.id, 
                            "⚠️ Название города не может быть пустым\n\nПожалуйста, введите корректное название населенного пункта\\."
                        )
                        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                        .await?;
                        return Ok(());
                    }
                }
            }
        }
        
        // Секретный код для активации "милого режима"
        // Используем необычную комбинацию символов, которую сложно угадать случайно
        if text.trim() == "<3cute<3" {
            // Получаем текущие настройки пользователя
            let mut user = storage.get_user(user_id).await.unwrap_or_else(|| UserSettings {
                user_id,
                city: None,
                notification_time: None,
                cute_mode: false,
                state: None,
            });
            
            // Включаем милый режим
            user.cute_mode = true;
            storage.save_user(user).await;
            
            bot.send_message(
                msg.chat.id, 
                "💕 *Милый режим активирован!*\n\nТеперь бот будет отправлять тебе милые сообщения и пожелания. Твой персональный бот-помощник всегда рядом!"
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
                state: None,
            });
            
            // Отключаем милый режим, если он был включен
            if user.cute_mode {
                user.cute_mode = false;
                storage.save_user(user).await;
                
                bot.send_message(
                    msg.chat.id, 
                    "🔄 Стандартный режим активирован. Бот будет отправлять только информативные сообщения о погоде."
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
            "Я понимаю только команды. Используйте /help для получения списка доступных команд."
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
        state: None,
    });
    
    // Принудительно устанавливаем стандартный режим при команде /start
    if user.cute_mode {
        user.cute_mode = false;
        storage.save_user(user).await;
    }
    
    // Всегда отправляем стандартное сообщение при /start
    let standard_text = "📱 Добро пожаловать в FerrisBot!\n\n\
                Я твой персональный бот-помощник с погодой! \
                Каждое утро я буду отправлять тебе актуальный прогноз погоды в указанное время.\n\n\
                Что я умею:\n\
                • 🌦️ Отправлять ежедневный прогноз погоды в твоем городе\n\
                • 🕒 Автоматически присылать прогноз в указанное время\n\
                • 🔍 Предоставлять прогноз по запросу в любое время\n\n\
                Для начала работы:\n\
                1️⃣ Сначала установи свой город командой /city\n\
                2️⃣ Затем установи время уведомлений: /time\n\
                3️⃣ Готово! Бот будет присылать прогноз погоды по расписанию\n\n\
                Важно: При вводе команд /city и /time можно выбрать вариант из меню или ввести значение вручную.\n\n\
                Другие команды:\n\
                /weather - получить текущий прогноз погоды\n\
                /forecast - получить прогноз погоды на неделю\n\
                /help - показать список всех команд";

    // Отправляем приветственное сообщение
    bot.send_message(msg.chat.id, standard_text)
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .await?;
    
    // Отправляем дополнительное сообщение с подсказкой
    bot.send_message(
        msg.chat.id,
        "👉 Пожалуйста, начните с установки вашего города командой /city"
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
        "✨ Доступные команды:\n\n\
         /start - начать работу с ботом\n\
         /help - показать это сообщение\n\
         /city - выбрать город из списка или ввести вручную\n\
         /time - выбрать время уведомлений из списка или ввести вручную\n\
         /weather - узнать текущую погоду\n\
         /forecast - получить прогноз погоды на неделю 💖\n\n\
         Совет: Команды /city и /time без параметров покажут интерактивное меню для выбора\\!"
    } else {
        "🌟 Доступные команды:\n\n\
         /start - начать работу с ботом\n\
         /help - показать это сообщение\n\
         /city - выбрать город из списка или ввести вручную\n\
         /time - выбрать время уведомлений из списка или ввести вручную\n\
         /weather - узнать текущую погоду\n\
         /forecast - получить прогноз погоды на неделю\n\n\
         Совет: Команды /city и /time без параметров покажут интерактивное меню для выбора\\!"
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
    
    // Если аргумент пустой, показываем клавиатуру выбора города
    if city_arg.trim().is_empty() {
        info!("Пользователь @{} запросил список городов", username);
        bot.send_message(
            msg.chat.id, 
            "🏙️ *Выберите город из списка или введите его вручную*\n\nДля ручного ввода используйте команду /city \\[название города\\]"
        )
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .reply_markup(get_city_keyboard())
        .await?;
        return Ok(());
    }
    
    // Специальная обработка для колбэка "manual"
    if city_arg.trim() == "manual" {
        bot.send_message(
            msg.chat.id, 
            "✏️ Пожалуйста, введите название вашего города после команды, например:\n/city Москва"
        ).await?;
        return Ok(());
    }

    let mut user = storage.get_user(user_id).await.unwrap_or_else(|| UserSettings {
        user_id,
        city: None,
        notification_time: None,
        cute_mode: false, // По умолчанию стандартный режим
        state: None,
    });

    // Сохраняем флаг cute_mode перед сохранением пользователя
    let is_cute_mode = user.cute_mode;
    
    user.city = Some(city_arg.trim().to_string());
    storage.save_user(user).await;
    
    info!("Пользователь @{} успешно установил город: {}", username, city_arg.trim());

    // Формируем сообщение в зависимости от режима
    let message = if is_cute_mode {
        format!("🌆 Город успешно установлен: {}\n\nТеперь ты можешь:\n• Узнать текущую погоду с помощью /weather\n• Установить время для ежедневных уведомлений командой /time", escape_markdown_v2(city_arg.trim()))
    } else {
        format!("🌆 Город успешно установлен: {}\n\nВы можете:\n• Узнать текущую погоду с помощью /weather\n• Установить время для ежедневных уведомлений командой /time", escape_markdown_v2(city_arg.trim()))
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
    
    // Если аргумент пустой, показываем клавиатуру выбора времени
    if time_arg.trim().is_empty() {
        info!("Пользователь @{} запросил список времени", username);
        bot.send_message(
            msg.chat.id, 
            "⏰ *Выберите время ежедневных уведомлений о погоде*\n\nДля ручного ввода используйте команду /time \\[ЧЧ:ММ\\]"
        )
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .reply_markup(get_time_keyboard())
        .await?;
        return Ok(());
    }

    // Специальная обработка для колбэка "manual"
    if time_arg.trim() == "manual" {
        bot.send_message(
            msg.chat.id, 
            "✏️ Пожалуйста, введите время в формате ЧЧ:ММ после команды, например:\n/time 08:00"
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
        cute_mode: false, // По умолчанию стандартный режим
        state: None,
    });

    // Сохраняем флаг cute_mode перед сохранением пользователя
    let is_cute_mode = user.cute_mode;
    
    user.notification_time = Some(time_arg.trim().to_string());
    storage.save_user(user).await;
    
    info!("Пользователь @{} успешно установил время уведомлений: {}", username, time_arg.trim());

    // Сообщение в зависимости от режима
    let message = if is_cute_mode {
        format!("⏰ Время уведомлений установлено: {}\n\nТеперь каждый день в это время я буду отправлять тебе прогноз погоды и милое сообщение! 💖", escape_markdown_v2(time_arg.trim()))
    } else {
        format!("⏰ Время уведомлений установлено: {}\n\nТеперь каждый день в это время вы будете получать актуальный прогноз погоды.", escape_markdown_v2(time_arg.trim()))
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
    
    if let Some(user_data) = user {
        match &user_data.city {
            Some(city) => {
                bot.send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing).await?;
                
                info!("Запрашиваю погоду для пользователя @{}, город: {}", username, city);
                
                match weather_client.get_weather(city).await {
                    Ok(weather) => {
                        info!("Успешно получена погода для пользователя @{}", username);
                        
                        // Формируем сообщение в зависимости от режима
                        let message = if user_data.cute_mode {
                            // Милый режим
                            format!("💖 Специально для тебя, погода в {}\n\n{}", 
                                escape_markdown_v2(city), 
                                escape_markdown_v2(&weather))
                        } else {
                            // Стандартный режим
                            format!("🌦️ Погода в {}\n\n{}", 
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
                            format!("❌ Не удалось получить погоду:\n{}\n\nПроверь правильность названия города или попробуй позже.", escape_markdown_v2(&e.to_string()))
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
                    "⚠️ Город не установлен\n\nПожалуйста, используй команду /city, чтобы установить город."
                )
                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                .await?;
            }
        }
    } else {
        info!("Пользователь @{} запросил погоду без настройки профиля", username);
        bot.send_message(
            msg.chat.id, 
            "⚠️ Требуется настройка\n\nПожалуйста, настрой бота с помощью команды /city."
        )
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .await?;
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
    
    if let Some(user_data) = user {
        match &user_data.city {
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
                        let message = if user_data.cute_mode {
                            // Милый режим
                            format!("✨ Прогноз погоды на неделю в {}\n\nСпециально для тебя я подготовил(а) детальный прогноз:\n\n{}", city_escaped, forecast_escaped)
                        } else {
                            // Стандартный режим
                            format!("🗓 Прогноз погоды на неделю в {}\n\n{}", city_escaped, forecast_escaped)
                        };
                        
                        bot.send_message(msg.chat.id, message)
                            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                            .await?;
                    }
                    Err(e) => {
                        error!("Ошибка получения прогноза на неделю для пользователя @{}: {}", username, e);
                        bot.send_message(
                            msg.chat.id, 
                            format!("❌ Не удалось получить прогноз:\n{}\n\nПроверь правильность названия города или попробуй позже.", escape_markdown_v2(&e.to_string()))
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
                    "⚠️ Город не установлен\n\nПожалуйста, используй команду /city, чтобы установить город."
                )
                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                .await?;
            }
        }
    } else {
        info!("Пользователь @{} запросил прогноз на неделю без настройки профиля", username);
        bot.send_message(
            msg.chat.id, 
            "⚠️ Требуется настройка\n\nПожалуйста, настрой бота с помощью команды /city."
        )
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .await?;
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

// Обработчик колбэков от инлайн-клавиатуры
async fn handle_callback_query(
    bot: Bot,
    q: CallbackQuery,
    storage: Arc<JsonStorage>,
) -> ResponseResult<()> {
    // Получаем ID пользователя
    if let Some(chat_id) = q.message.as_ref().map(|msg| msg.chat.id) {
        let user_id = chat_id.0;
        
        if let Some(data) = q.data {
            if data.starts_with("city_") {
                if data == "city_manual" {
                    // Пользователь выбрал ручной ввод города
                    // Устанавливаем состояние ожидания ввода города
                    let mut user = storage.get_user(user_id).await.unwrap_or_else(|| UserSettings {
                        user_id,
                        city: None,
                        notification_time: None,
                        cute_mode: false,
                        state: None,
                    });
                    
                    user.state = Some("waiting_for_city".to_string());
                    storage.save_user(user).await;
                    
                    bot.answer_callback_query(q.id).await?;
                    
                    if let Some(message_id) = q.message.as_ref().map(|msg| msg.id) {
                        bot.edit_message_text(chat_id, message_id, 
                            "🏙️ Ввод города вручную\n\nПожалуйста, напишите название вашего города.\n\nПримеры: Москва, Санкт-Петербург, Новосибирск"
                        )
                        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                        .await?;
                    }
                    
                    return Ok(());
                }
                
                // Обрабатываем выбор города из меню
                let city = data.replace("city_", "");
                
                // Получаем или создаем настройки пользователя
                let mut user = storage.get_user(user_id).await.unwrap_or_else(|| UserSettings {
                    user_id,
                    city: None,
                    notification_time: None,
                    cute_mode: false,
                    state: None,
                });
                
                let is_cute_mode = user.cute_mode;
                user.city = Some(city.clone());
                user.state = None; // Сбрасываем состояние, если оно было
                storage.save_user(user).await;
                
                // Формируем сообщение
                let message = if is_cute_mode {
                    format!("🌆 Город успешно установлен: {}\n\nТеперь ты можешь:\n• Узнать текущую погоду с помощью /weather\n• Установить время для ежедневных уведомлений командой /time", escape_markdown_v2(&city))
                } else {
                    format!("🌆 Город успешно установлен: {}\n\nВы можете:\n• Узнать текущую погоду с помощью /weather\n• Установить время для ежедневных уведомлений командой /time", escape_markdown_v2(&city))
                };
                
                // Отвечаем на колбэк
                bot.answer_callback_query(q.id).await?;
                
                // Редактируем сообщение с инлайн-клавиатурой
                if let Some(message_id) = q.message.as_ref().map(|msg| msg.id) {
                    bot.edit_message_text(chat_id, message_id, message)
                        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                        .await?;
                }
                
                info!("Пользователь ID: {} выбрал город: {} через меню", user_id, city);
            } else if data.starts_with("time_") {
                if data == "time_manual" {
                    // Пользователь выбрал ручной ввод времени
                    // Устанавливаем состояние ожидания ввода времени
                    let mut user = storage.get_user(user_id).await.unwrap_or_else(|| UserSettings {
                        user_id,
                        city: None,
                        notification_time: None,
                        cute_mode: false,
                        state: None,
                    });
                    
                    user.state = Some("waiting_for_time".to_string());
                    storage.save_user(user).await;
                    
                    bot.answer_callback_query(q.id).await?;
                    
                    if let Some(message_id) = q.message.as_ref().map(|msg| msg.id) {
                        bot.edit_message_text(chat_id, message_id, 
                            "⏰ Ввод времени вручную\n\nПожалуйста, напишите время в формате ЧЧ:ММ, например: 08:30\n\nДопустимое время: от 00:00 до 23:59"
                        )
                        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                        .await?;
                    }
                    
                    return Ok(());
                }
                
                // Обрабатываем выбор времени из меню
                let time = data.replace("time_", "");
                
                // Получаем или создаем настройки пользователя
                let mut user = storage.get_user(user_id).await.unwrap_or_else(|| UserSettings {
                    user_id,
                    city: None,
                    notification_time: None,
                    cute_mode: false,
                    state: None,
                });
                
                let is_cute_mode = user.cute_mode;
                user.notification_time = Some(time.clone());
                user.state = None; // Сбрасываем состояние, если оно было
                storage.save_user(user).await;
                
                // Формируем сообщение
                let message = if is_cute_mode {
                    format!("⏰ Время уведомлений установлено: {}\n\nТеперь каждый день в это время я буду отправлять тебе прогноз погоды и милое сообщение! 💖", escape_markdown_v2(&time))
                } else {
                    format!("⏰ Время уведомлений установлено: {}\n\nТеперь каждый день в это время вы будете получать актуальный прогноз погоды.", escape_markdown_v2(&time))
                };
                
                // Отвечаем на колбэк
                bot.answer_callback_query(q.id).await?;
                
                // Редактируем сообщение с инлайн-клавиатурой
                if let Some(message_id) = q.message.as_ref().map(|msg| msg.id) {
                    bot.edit_message_text(chat_id, message_id, message)
                        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                        .await?;
                }
                
                info!("Пользователь ID: {} выбрал время: {} через меню", user_id, time);
            }
        }
    }
    
    Ok(())
}

// Получение списка популярных городов России
fn get_city_keyboard() -> InlineKeyboardMarkup {
    let mut keyboard: Vec<Vec<InlineKeyboardButton>> = vec![];
    
    let cities = [
        "Москва", "Санкт-Петербург", "Новосибирск", "Екатеринбург", 
        "Тюмень", "Нижний Новгород", "Челябинск", "Самара", 
        "Омск", "Ростов-на-Дону", "Уфа", "Красноярск", 
        "Воронеж", "Пермь", "Волгоград"
    ];
    
    for chunk in cities.chunks(3) {
        let row = chunk.iter()
            .map(|city| {
                InlineKeyboardButton::callback(city.to_string(), format!("city_{}", city))
            })
            .collect();
        keyboard.push(row);
    }
    
    // Добавляем напоминание о ручном вводе
    keyboard.push(vec![
        InlineKeyboardButton::callback("Ввести город вручную".to_string(), "city_manual".to_string())
    ]);
    
    InlineKeyboardMarkup::new(keyboard)
}

// Получение клавиатуры для выбора времени
fn get_time_keyboard() -> InlineKeyboardMarkup {
    let mut keyboard: Vec<Vec<InlineKeyboardButton>> = vec![];
    
    // Утреннее время
    let morning = vec![
        InlineKeyboardButton::callback("06:00".to_string(), "time_06:00".to_string()),
        InlineKeyboardButton::callback("07:00".to_string(), "time_07:00".to_string()),
        InlineKeyboardButton::callback("08:00".to_string(), "time_08:00".to_string()),
        InlineKeyboardButton::callback("09:00".to_string(), "time_09:00".to_string()),
    ];
    
    // Дневное время
    let day = vec![
        InlineKeyboardButton::callback("12:00".to_string(), "time_12:00".to_string()),
        InlineKeyboardButton::callback("14:00".to_string(), "time_14:00".to_string()),
        InlineKeyboardButton::callback("16:00".to_string(), "time_16:00".to_string()),
    ];
    
    // Вечернее время
    let evening = vec![
        InlineKeyboardButton::callback("18:00".to_string(), "time_18:00".to_string()),
        InlineKeyboardButton::callback("20:00".to_string(), "time_20:00".to_string()),
        InlineKeyboardButton::callback("22:00".to_string(), "time_22:00".to_string()),
    ];
    
    keyboard.push(morning);
    keyboard.push(day);
    keyboard.push(evening);
    
    // Добавляем напоминание о ручном вводе
    keyboard.push(vec![
        InlineKeyboardButton::callback("Ввести время вручную".to_string(), "time_manual".to_string())
    ]);
    
    InlineKeyboardMarkup::new(keyboard)
}
