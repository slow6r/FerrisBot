use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::io::ErrorKind;
use log::error;
use log::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettings {
    pub user_id: i64,
    pub city: Option<String>,
    pub notification_time: Option<String>,
    pub cute_mode: bool, // Флаг указывающий использует ли пользователь "милый режим"
    pub state: Option<String>, // Добавляем поле для хранения состояния пользователя
}

#[derive(Clone)]
pub struct JsonStorage {
    pub data: Arc<RwLock<Vec<UserSettings>>>,
    file_path: String,
}

impl JsonStorage {
    pub async fn new(path: &str) -> Self {
        // Создаем хранилище и пытаемся загрузить существующие данные
        let data = match fs::read_to_string(path) {
            Ok(content) => {
                if content.trim().is_empty() {
                    // Файл пустой, начинаем с пустого списка
                    info!("Файл данных пустой, создан новый список пользователей");
                    Vec::new()
                } else {
                    match serde_json::from_str::<Vec<UserSettings>>(&content) {
                        Ok(users) => users,
                        Err(e) => {
                            error!("Ошибка десериализации данных: {}", e);
                            // Создаем резервную копию проблемного файла
                            let backup_path = format!("{}.backup", path);
                            if let Err(copy_err) = fs::copy(path, &backup_path) {
                                error!("Не удалось создать резервную копию: {}", copy_err);
                            } else {
                                info!("Создана резервная копия поврежденного файла данных: {}", backup_path);
                            }
                            Vec::new()
                        }
                    }
                }
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                // Файл не найден, начинаем с пустого списка
                info!("Файл данных не найден, создан новый файл: {}", path);
                Vec::new()
            }
            Err(e) => {
                error!("Ошибка чтения файла: {}", e);
                Vec::new()
            }
        };

        JsonStorage {
            data: Arc::new(RwLock::new(data)),
            file_path: path.to_string(),
        }
    }

    pub async fn get_user(&self, user_id: i64) -> Option<UserSettings> {
        let data = self.data.read().await;
        data.iter().find(|user| user.user_id == user_id).cloned()
    }

    pub async fn save_user(&self, user: UserSettings) {
        let mut data = self.data.write().await;
        if let Some(pos) = data.iter().position(|u| u.user_id == user.user_id) {
            data[pos] = user;
        } else {
            data.push(user);
        }
        
        // Сохраняем обновленные данные в файл
        self.save_to_file(&data).await;
    }

    pub async fn get_all_users(&self) -> Vec<UserSettings> {
        let data = self.data.read().await;
        data.clone()
    }
    
    async fn save_to_file(&self, data: &[UserSettings]) {
        match serde_json::to_string_pretty(data) {
            Ok(json) => {
                if let Err(e) = fs::write(&self.file_path, json) {
                    error!("Ошибка сохранения данных в файл: {}", e);
                }
            }
            Err(e) => {
                error!("Ошибка сериализации данных: {}", e);
            }
        }
    }
}
