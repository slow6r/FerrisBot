// Вспомогательные функции для работы с ботом
use teloxide::prelude::*;
use teloxide::types::{ParseMode, ReplyMarkup};

// Функция экранирования специальных символов Markdown V2
pub fn escape_markdown_v2(text: &str) -> String {
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

// Вспомогательная функция для отправки сообщений с MarkdownV2
// Гарантирует правильное экранирование всех специальных символов
pub async fn send_markdown_message<'a>(
    bot: &Bot, 
    chat_id: ChatId, 
    text: &str, 
    reply_markup: Option<impl Into<ReplyMarkup> + Send>
) -> ResponseResult<Message> {
    // Применяем экранирование к всему тексту
    let escaped_text = escape_markdown_v2(text);
    
    // Создаем базовое сообщение
    let mut message_builder = bot.send_message(chat_id, escaped_text)
        .parse_mode(ParseMode::MarkdownV2);
    
    // Добавляем разметку, если она предоставлена
    if let Some(markup) = reply_markup {
        message_builder = message_builder.reply_markup(markup);
    }
    
    // Отправляем сообщение
    message_builder.await
} 