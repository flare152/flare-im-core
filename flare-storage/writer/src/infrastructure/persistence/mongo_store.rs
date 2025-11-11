use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use mongodb::bson::{Document, doc};
use mongodb::options::{ClientOptions, IndexOptions, UpdateOptions};
use mongodb::{Client, Collection, IndexModel};

use crate::domain::repositories::RealtimeStoreRepository;
use crate::infrastructure::config::StorageWriterConfig;

pub struct MongoMessageStore {
    collection: Collection<Document>,
    _client: Arc<Client>,
}

impl MongoMessageStore {
    pub async fn new(config: &StorageWriterConfig) -> Result<Option<Self>> {
        let uri = match &config.mongo_url {
            Some(url) => url,
            None => return Ok(None),
        };

        let options = ClientOptions::parse(uri).await?;
        let client = Arc::new(Client::with_options(options)?);
        let database = client.database(&config.mongo_database);
        let collection = database.collection::<Document>(&config.mongo_collection);

        ensure_indexes(&collection).await?;

        Ok(Some(Self {
            collection,
            _client: client,
        }))
    }
}

async fn ensure_indexes(collection: &Collection<Document>) -> Result<()> {
    let message_index = IndexModel::builder()
        .keys(doc! {"message_id": 1})
        .options(
            IndexOptions::builder()
                .unique(true)
                .name(Some("uid_message".to_string()))
                .build(),
        )
        .build();
    collection
        .create_index(message_index, None::<mongodb::options::CreateIndexOptions>)
        .await?;

    let session_index = IndexModel::builder()
        .keys(doc! {"session_id": 1, "ingestion_ts": -1})
        .options(
            IndexOptions::builder()
                .name(Some("idx_session_ingestion".to_string()))
                .build(),
        )
        .build();
    collection
        .create_index(session_index, None::<mongodb::options::CreateIndexOptions>)
        .await?;

    Ok(())
}

#[async_trait]
impl RealtimeStoreRepository for MongoMessageStore {
    async fn store_realtime(&self, stored: &flare_storage_model::StoredMessage) -> Result<()> {
        let stored_doc = mongodb::bson::to_document(stored)?;
        let document = doc! {
            "message_id": &stored.envelope.message_id,
            "session_id": &stored.envelope.session_id,
            "ingestion_ts": stored.timeline.ingestion_ts,
            "stored": stored_doc,
        };

        let filter = doc! {"message_id": &stored.envelope.message_id};
        let update = doc! {"$set": document};
        let options = UpdateOptions::builder().upsert(true).build();

        self.collection.update_one(filter, update, options).await?;
        Ok(())
    }
}
