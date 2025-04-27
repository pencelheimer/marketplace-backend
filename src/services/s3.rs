use aws_config::BehaviorVersion;
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use aws_types::region::Region;
use once_cell::sync::Lazy;
use std::env;
use uuid::Uuid;

pub(crate) const MAX_FILE_SIZE: usize = 5 * 1024 * 1024;

pub static AWS_MARKETPLACE_BUCKET: Lazy<String> =
    Lazy::new(|| env::var("AWS_MARKETPLACE_BUCKET").expect("AWS_MARKETPLACE_BUCKET not set"));

pub static AWS_REGION: Lazy<String> =
    Lazy::new(|| env::var("AWS_REGION").expect("AWS_REGION not set"));

pub(crate) async fn upload_to_s3(
    bucket: &str,
    file_bytes: Vec<u8>,
    filename: &str,
) -> Result<String, actix_web::Error> {
    let region_provider = RegionProviderChain::first_try(Some(Region::new(AWS_REGION.as_str())))
        .or_default_provider();

    let config = aws_config::defaults(BehaviorVersion::latest())
        .region(region_provider)
        .load()
        .await;

    let client = Client::new(&config);

    let key = format!(
        "uploads/{}-{}",
        Uuid::new_v4(),
        sanitize_filename::sanitize(filename)
    );

    let body = ByteStream::from(file_bytes);

    client
        .put_object()
        .bucket(bucket)
        .key(&key)
        .body(body)
        .send()
        .await
        .map_err(|e| {
            eprintln!("S3 Upload Error: {}", e);
            actix_web::error::ErrorInternalServerError("Failed to upload to S3")
        })?;

    let url = format!("https://{}.s3.amazonaws.com/{}", bucket, key);

    Ok(url)
}
