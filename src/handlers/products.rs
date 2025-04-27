use crate::handlers::auth::AuthenticatedUser;
use crate::services::s3::{AWS_MARKETPLACE_BUCKET, AWS_REGION, MAX_FILE_SIZE, upload_to_s3};
use actix_multipart::Multipart;
use actix_web::{HttpResponse, Responder, get, post, web};
use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use futures_util::StreamExt;
use mime_guess::from_path;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::{Arguments, FromRow, PgPool, Postgres, QueryBuilder, Row, Transaction};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

#[derive(Serialize, Deserialize, sqlx::FromRow)]
struct Category {
    category_id: i32,
    name: String,
    photo: String,
}

#[derive(Serialize, Deserialize)]
struct CategoriesResponse {
    categories: Vec<Category>,
}

#[get("/categories")]
async fn categories(db_pool: web::Data<PgPool>) -> Result<impl Responder, actix_web::Error> {
    let rows = sqlx::query_as::<_, Category>(
        "SELECT category_id, name, photo FROM categories ORDER BY name",
    )
    .fetch_all(db_pool.get_ref())
    .await
    .map_err(actix_web::error::ErrorInternalServerError)?;

    let categories: Vec<Category> = rows
        .into_iter()
        .map(|mut c| {
            c.photo = format!(
                "https://{}.s3.{}.amazonaws.com/media/{}",
                AWS_MARKETPLACE_BUCKET.as_str(),
                AWS_REGION.as_str(),
                c.photo
            );
            c
        })
        .collect();

    Ok(HttpResponse::Ok()
        .content_type("application/json; charset=utf-8")
        .json(CategoriesResponse { categories }))
}

#[derive(Serialize, Deserialize, sqlx::FromRow)]
struct PaymentOptions {
    id: i32,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct PaymentOptionsRequest {
    payment_options: Vec<PaymentOptions>,
}

#[get("/payment-options")]
async fn payment_options(db_pool: web::Data<PgPool>) -> Result<impl Responder, actix_web::Error> {
    let payment_options =
        sqlx::query_as::<_, PaymentOptions>("SELECT id, name FROM payment_options ORDER BY id")
            .fetch_all(db_pool.get_ref())
            .await
            .map_err(actix_web::error::ErrorInternalServerError)?;

    Ok(HttpResponse::Ok()
        .content_type("application/json; charset=utf-8")
        .json(PaymentOptionsRequest { payment_options }))
}

#[derive(Serialize, Deserialize, sqlx::FromRow)]
struct DeliveryOptions {
    id: i32,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct DeliveryOptionsRequest {
    delivery_options: Vec<PaymentOptions>,
}

#[get("/delivery-options")]
async fn delivery_options(db_pool: web::Data<PgPool>) -> Result<impl Responder, actix_web::Error> {
    let delivery_options =
        sqlx::query_as::<_, PaymentOptions>("SELECT id, name FROM delivery_options ORDER BY id")
            .fetch_all(db_pool.get_ref())
            .await
            .map_err(actix_web::error::ErrorInternalServerError)?;

    Ok(HttpResponse::Ok()
        .content_type("application/json; charset=utf-8")
        .json(DeliveryOptionsRequest { delivery_options }))
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ProductCondition {
    NEW,
    USED,
}

impl fmt::Display for ProductCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            ProductCondition::NEW => write!(f, "NEW"),
            ProductCondition::USED => write!(f, "USED"),
        }
    }
}

impl FromStr for ProductCondition {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "NEW" => Ok(ProductCondition::NEW),
            "USED" => Ok(ProductCondition::USED),
            _ => Err(()),
        }
    }
}

#[derive(Deserialize)]
pub struct CreateProductRequest {
    pub title: String,
    pub description: String,
    pub category_id: i32,
    pub brand: Option<String>,
    pub condition: ProductCondition,
    pub price: f64,
    pub phone_number: String,
    pub delivery_option_ids: Vec<i32>,
    pub payment_option_ids: Vec<i32>,
}

pub fn validate_phone_number(phone_number: &str) -> Result<(), actix_web::Error> {
    let phone_number_regex = Regex::new(r"^(\+380\d{9}|\d{10})$").unwrap();

    if !phone_number_regex.is_match(phone_number) {
        Err(actix_web::error::ErrorBadRequest(
            "Invalid phone number format",
        ))
    } else {
        Ok(())
    }
}

#[derive(Serialize)]
pub struct CreateProductResponse {
    pub product_id: i32,
}

fn parse_form_data(
    form: HashMap<String, String>,
) -> Result<CreateProductRequest, actix_web::Error> {
    let title = form
        .get("title")
        .ok_or_else(|| actix_web::error::ErrorBadRequest("Missing title"))?
        .clone();
    let description = form
        .get("description")
        .ok_or_else(|| actix_web::error::ErrorBadRequest("Missing description"))?
        .clone();
    let phone_number = form
        .get("phone_number")
        .ok_or_else(|| actix_web::error::ErrorBadRequest("Phone number is missing"))?
        .clone();

    validate_phone_number(&phone_number)?;

    let price = form
        .get("price")
        .ok_or(actix_web::error::ErrorBadRequest("Missing price"))?
        .parse::<f64>()
        .map_err(|_| actix_web::error::ErrorBadRequest("Invalid price format"))?;

    let category_id = form
        .get("category_id")
        .ok_or(actix_web::error::ErrorBadRequest("Missing category"))?
        .parse::<i32>()
        .map_err(|_| actix_web::error::ErrorBadRequest("Invalid price format"))?;

    let delivery_option_ids = form
        .get("delivery_option")
        .map(|v| v.split(',').map(|s| s.parse::<i32>().unwrap()).collect())
        .unwrap_or_else(|| vec![]);

    let payment_option_ids = form
        .get("payment_option")
        .map(|v| v.split(',').map(|s| s.parse::<i32>().unwrap()).collect())
        .unwrap_or_else(|| vec![]);

    let brand = form.get("brand").cloned();

    let condition = form
        .get("condition")
        .ok_or_else(|| actix_web::error::ErrorBadRequest("Missing condition"))?
        .parse::<ProductCondition>()
        .map_err(|_| actix_web::error::ErrorBadRequest("Invalid condition"))?;

    Ok(CreateProductRequest {
        title,
        description,
        category_id,
        brand,
        condition,
        price,
        phone_number,
        delivery_option_ids,
        payment_option_ids,
    })
}

async fn insert_product(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    user_id: &Uuid,
    data: &CreateProductRequest,
) -> Result<i32, actix_web::Error> {
    let rec = sqlx::query(
        "INSERT INTO products
        (user_id, title, description, category_id, brand, condition, price, phone_number)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id",
    )
    .bind(user_id)
    .bind(&data.title)
    .bind(&data.description)
    .bind(&data.category_id)
    .bind(&data.brand)
    .bind(&data.condition.to_string())
    .bind(&data.price)
    .bind(&data.phone_number)
    .fetch_one(&mut **tx)
    .await
    .map_err(actix_web::error::ErrorInternalServerError)?;

    Ok(rec
        .try_get("id")
        .map_err(actix_web::error::ErrorInternalServerError)?)
}

async fn insert_product_options(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    product_id: i32,
    data: &CreateProductRequest,
) -> Result<(), actix_web::Error> {
    if !data.delivery_option_ids.is_empty() {
        let mut builder = QueryBuilder::new(
            "INSERT INTO product_delivery_options (product_id, delivery_option_id) ",
        );
        builder.push_values(
            data.delivery_option_ids.iter().map(|id| (product_id, *id)),
            |mut b, (pid, did)| {
                b.push_bind(pid).push_bind(did);
            },
        );
        builder
            .build()
            .execute(&mut **tx)
            .await
            .map_err(actix_web::error::ErrorInternalServerError)?;
    }

    if !data.payment_option_ids.is_empty() {
        let mut builder = QueryBuilder::new(
            "INSERT INTO product_payment_options (product_id, payment_option_id) ",
        );
        builder.push_values(
            data.payment_option_ids.iter().map(|id| (product_id, *id)),
            |mut b, (pid, pid_opt)| {
                b.push_bind(pid).push_bind(pid_opt);
            },
        );
        builder
            .build()
            .execute(&mut **tx)
            .await
            .map_err(actix_web::error::ErrorInternalServerError)?;
    }

    Ok(())
}

async fn insert_product_photo(
    tx: &mut Transaction<'_, Postgres>,
    product_id: i32,
    photo_url: &str,
    position: i32,
) -> Result<(), actix_web::Error> {
    sqlx::query("INSERT INTO product_images (product_id, url, position) VALUES ($1, $2, $3)")
        .bind(product_id)
        .bind(photo_url)
        .bind(position)
        .execute(&mut **tx)
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;
    Ok(())
}

#[post("/create")]
pub async fn create(
    user: AuthenticatedUser,
    mut payload: Multipart,
    db_pool: web::Data<PgPool>,
) -> Result<impl Responder, actix_web::Error> {
    let user_id = &user.0.sub;

    let mut form_data = HashMap::new();
    let mut photos = Vec::new();

    while let Some(field) = payload.next().await {
        let mut field = field.map_err(actix_web::error::ErrorInternalServerError)?;
        let name = field
            .content_disposition()
            .unwrap()
            .get_name()
            .unwrap()
            .to_string();

        if name == "photos" {
            let filename = field
                .content_disposition()
                .unwrap()
                .get_filename()
                .map(sanitize_filename::sanitize)
                .unwrap_or_else(|| "upload.jpg".to_string());

            let mut bytes = Vec::new();
            while let Some(chunk) = field.next().await {
                let data = chunk.map_err(actix_web::error::ErrorInternalServerError)?;
                bytes.extend_from_slice(&data);
                if bytes.len() > MAX_FILE_SIZE {
                    return Err(actix_web::error::ErrorBadRequest("File too large"));
                }
            }

            let mime = from_path(&filename).first_or_octet_stream();
            if !matches!(
                mime.essence_str(),
                "image/jpeg" | "image/png" | "image/jpg" | "image/webp"
            ) {
                return Err(actix_web::error::ErrorBadRequest("Invalid file type"));
            }

            photos.push((bytes, filename));
        } else {
            let mut value = Vec::new();
            while let Some(chunk) = field.next().await {
                value.extend_from_slice(&chunk?);
            }
            form_data.insert(name, String::from_utf8_lossy(&value).to_string());
        }
    }

    let data = parse_form_data(form_data)?;

    if photos.is_empty() {
        return Err(actix_web::error::ErrorBadRequest(
            "At least one photo is required",
        ));
    }

    let mut tx = db_pool
        .begin()
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    let product_id = insert_product(&mut tx, user_id, &data).await?;

    for (index, (photo_bytes, photo_filename)) in photos.into_iter().enumerate() {
        let photo_url = upload_to_s3(
            AWS_MARKETPLACE_BUCKET.as_str(),
            photo_bytes,
            &photo_filename,
        )
        .await?;

        insert_product_photo(&mut tx, product_id, &photo_url, index as i32).await?;
    }

    insert_product_options(&mut tx, product_id, &data).await?;

    tx.commit()
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    Ok(HttpResponse::Ok().body("Product created"))
}

#[derive(Deserialize)]
pub struct ProductQuery {
    category: Option<String>,
    last_seen_id: Option<i64>,
    limit: Option<i64>,
}

#[derive(FromRow, Serialize)]
pub struct Product {
    id: i32,
    photo: String,
    title: String,
    category_id: i32,
    description: String,
    brand: Option<String>,
    condition: String,
    price: BigDecimal,
    phone_number: String,
    created_at: NaiveDateTime,
}

#[get("")]
pub async fn get_products(
    user: AuthenticatedUser,
    pool: web::Data<PgPool>,
    query: web::Query<ProductQuery>,
) -> Result<HttpResponse, actix_web::Error> {
    println!("Authenticated user: {:?}", user);

    let mut sql = String::from(
        "SELECT id, photo, title, category_id, description, brand, condition, price, phone_number, created_at FROM products WHERE 1=1",
    );
    let mut args = sqlx::postgres::PgArguments::default();
    let mut arg_count = 1;

    if let Some(ref category) = query.category {
        sql.push_str(" AND category = $1");
        args.add(category).map_err(|e| {
            eprintln!("Argument error: {}", e);
            <actix_web::Error as std::convert::From<_>>::from(actix_web::error::InternalError::new(
                e,
                actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        })?;
        arg_count += 1;
    }

    if let Some(last_seen_id) = query.last_seen_id {
        sql.push_str(" AND id < $2");
        args.add(last_seen_id).map_err(|e| {
            eprintln!("Argument error: {}", e);
            <actix_web::Error as std::convert::From<_>>::from(actix_web::error::InternalError::new(
                e,
                actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        })?;
        arg_count += 1;
    }

    sql.push_str(" ORDER BY id DESC");

    let limit = query.limit.unwrap_or(20);
    sql.push_str(&format!(" LIMIT ${}", arg_count));
    args.add(limit).map_err(|e| {
        eprintln!("Argument error: {}", e);
        <actix_web::Error as std::convert::From<_>>::from(actix_web::error::InternalError::new(
            e,
            actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
        ))
    })?;

    println!("SQL: {}", sql);

    println!("Args: {:?}", args);

    let products = sqlx::query_as_with::<_, Product, _>(&sql, args)
        .fetch_all(pool.get_ref())
        .await
        .map_err(|e| {
            eprintln!("Database error: {}", e);
            <actix_web::Error as std::convert::From<_>>::from(actix_web::error::InternalError::new(
                e,
                actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        })?;

    Ok(HttpResponse::Ok().json(products))
}
