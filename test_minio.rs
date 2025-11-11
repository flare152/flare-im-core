use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::{config::Builder as S3ConfigBuilder, config::Credentials, config::Region, Client as S3Client};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bucket = "flare-media";
    let region = Region::new("us-east-1");
    let endpoint = "http://localhost:29000";
    let access_key = "minioadmin";
    let secret_key = "minioadmin";

    // Build base AWS config
    let region_provider = RegionProviderChain::first_try(region.clone());
    let loader = aws_config::from_env().region(region_provider);
    
    // Credentials (access_key/secret_key), if provided, use static
    let credentials = Credentials::new(
        access_key,
        secret_key,
        None,
        None,
        "static-credentials",
    );
    let aws_cfg = loader.credentials_provider(credentials).load().await;

    // Build S3 client config
    let mut s3_builder = S3ConfigBuilder::from(&aws_cfg).region(region);
    s3_builder = s3_builder.endpoint_url(endpoint);
    s3_builder = s3_builder.force_path_style(true);
    let s3_config = s3_builder.build();
    let client = S3Client::from_conf(s3_config);

    println!("Checking if bucket '{}' exists...", bucket);
    
    match client.list_buckets().send().await {
        Ok(output) => {
            println!("Successfully connected to MinIO!");
            if let Some(buckets) = output.buckets {
                println!("Found {} buckets:", buckets.len());
                for bucket in buckets {
                    if let Some(name) = bucket.name {
                        println!("  - {}", name);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to list buckets: {}", e);
        }
    }

    // Check if our specific bucket exists
    match client.head_bucket().bucket(bucket).send().await {
        Ok(_) => {
            println!("Bucket '{}' exists!", bucket);
        }
        Err(e) => {
            eprintln!("Bucket '{}' does not exist or is not accessible: {}", bucket, e);
        }
    }

    Ok(())
}