pub mod id;

use crate::{db::id::IdGenerator, metrics::MetricsCollector};

use std::collections::HashMap;
use tokio::sync::RwLock;

#[derive(Debug, Default, Clone)]
pub struct Greeting {
    pub id: u32,
    pub sender_name: String,
    pub message: String,
    pub timestamp: jiff::Timestamp,
}

#[derive(Debug, Default)]
pub struct NewGreeting {
    pub recipient_name: String,
    pub sender_name: String,
    pub message: String,
}

// TODO: move this to a real db
#[derive(Debug, Default)]
pub struct GreetingsRepository {
    ids: RwLock<HashMap<String, Vec<Greeting>>>,
    id_generator: IdGenerator,
}

impl GreetingsRepository {
    pub fn new() -> Self {
        Self {
            ids: RwLock::new(HashMap::new()),
            id_generator: IdGenerator::default(),
        }
    }

    pub async fn add_new_greeting(&self, greeting: NewGreeting) {
        let NewGreeting {
            recipient_name,
            sender_name,
            message,
        } = greeting;

        let mut guard = self.ids.write().await;

        guard.entry(recipient_name).or_default().push(Greeting {
            id: self.id_generator.next_id(),
            sender_name,
            message,
            timestamp: jiff::Timestamp::now(),
        });
    }

    pub async fn get_greetings_by_name(&self, name: &str) -> Option<Vec<Greeting>> {
        let guard = self.ids.read().await;

        guard.get(name).cloned()
    }

    pub async fn get_all_greetings(&self) -> Vec<(String, Greeting)> {
        let guard = self.ids.read().await;

        guard
            .iter()
            .flat_map(|(recipient_name, greetings)| {
                greetings
                    .iter()
                    .cloned()
                    .map(|greeting| (recipient_name.clone(), greeting))
            })
            .collect()
    }
}

impl MetricsCollector for GreetingsRepository {
    async fn collect(&self) {
        let guard = self.ids.read().await;
        let total_greetings = guard.values().map(Vec::len).sum::<usize>();

        metrics::gauge!("greetings_stored_total").set(total_greetings as f64);
        metrics::gauge!("greeting_recipients_total").set(guard.len() as f64);
    }
}
