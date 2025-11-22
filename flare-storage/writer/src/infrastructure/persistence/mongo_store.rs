use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use mongodb::bson::{Document, doc};
use mongodb::options::{ClientOptions, IndexOptions, UpdateOptions};
use mongodb::{Client, Collection, IndexModel};
use prost::Message as _;

use crate::config::StorageWriterConfig;
use crate::domain::repository::RealtimeStoreRepository;

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
    async fn store_realtime(&self, message: &flare_proto::common::Message) -> Result<()> {
        // 将 Message 编码为 BSON
        let mut buf = Vec::new();
        message.encode(&mut buf)?;
        let message_doc = mongodb::bson::Binary {
            subtype: mongodb::bson::spec::BinarySubtype::Generic,
            bytes: buf,
        };

        // 从 extra 中提取 ingestion_ts
        let ingestion_ts = flare_im_core::utils::extract_timeline_from_extra(
            &message.extra,
            flare_im_core::utils::current_millis(),
        )
        .ingestion_ts;

        let document = doc! {
            "message_id": &message.id,
            "session_id": &message.session_id,
            "ingestion_ts": ingestion_ts,
            "message": message_doc,
        };

        let filter = doc! {"message_id": &message.id};
        let update = doc! {"$set": document};
        let options = UpdateOptions::builder().upsert(true).build();

        self.collection.update_one(filter, update, options).await?;
        Ok(())
    }
}
