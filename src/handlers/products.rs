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
use sqlx::types::Json;
use sqlx::{FromRow, PgPool, Postgres, QueryBuilder, Row, Transaction};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

#[derive(Serialize, Deserialize, FromRow)]
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

#[derive(Serialize, Deserialize, FromRow)]
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

#[derive(Serialize, Deserialize, FromRow)]
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
    pub color: Option<String>,
    pub shoe_size: Option<String>,
    pub clothing_size: Option<String>,
    pub gender: Option<String>,
    pub material: Option<String>,
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

    let color = form.get("color").cloned();
    let shoe_size = form.get("shoe_size").cloned();
    let clothing_size = form.get("clothing_size").cloned();
    let gender = form.get("gender").cloned();
    let material = form.get("material").cloned();

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
        color,
        shoe_size,
        clothing_size,
        gender,
        material,
    })
}

async fn insert_product(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    user_id: &Uuid,
    data: &CreateProductRequest,
) -> Result<i32, actix_web::Error> {
    let rec = sqlx::query(
        "INSERT INTO products
        (user_id, title, description, category_id, brand, condition, price, phone_number,
         color, shoe_size, clothing_size, gender, material)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8,
                $9, $10, $11, $12, $13)
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
    .bind(&data.color)
    .bind(&data.shoe_size)
    .bind(&data.clothing_size)
    .bind(&data.gender)
    .bind(&data.material)
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
    user_id: Option<Uuid>,
    search: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Photo {
    id: i32,
    url: String,
}

#[derive(FromRow, Serialize)]
pub struct Product {
    id: i32,
    title: String,
    category_id: i32,
    description: String,
    brand: Option<String>,
    condition: String,
    price: BigDecimal,
    phone_number: String,
    created_at: NaiveDateTime,
    user_id: Uuid,
    color: Option<String>,
    shoe_size: Option<String>,
    clothing_size: Option<String>,
    gender: Option<String>,
    material: Option<String>,
    photos: Json<Vec<Photo>>,
}

#[get("")]
pub async fn get_products(
    pool: web::Data<PgPool>,
    query: web::Query<ProductQuery>,
) -> Result<HttpResponse, actix_web::Error> {
    let limit = query.limit.unwrap_or(20);

    let mut qb = QueryBuilder::new(
        r#"
    SELECT
        p.id,
        p.title,
        p.category_id,
        p.description,
        p.brand,
        p.condition,
        p.price,
        p.phone_number,
        p.created_at,
        p.user_id,
        p.color,
        p.shoe_size,
        p.clothing_size,
        p.gender,
        p.material,
        COALESCE(
            json_agg(
                json_build_object('id', ph.id, 'url', ph.url)
            ) FILTER (WHERE ph.id IS NOT NULL),
            '[]'
        )::json AS photos
    FROM products p
    LEFT JOIN product_images ph ON ph.product_id = p.id
    WHERE 1=1
"#,
    );

    if let Some(category_id) = &query.category {
        qb.push(" AND p.category_id = ");
        qb.push_bind(category_id);
    }

    if let Some(user_id) = &query.user_id {
        qb.push(" AND p.user_id = ");
        qb.push_bind(user_id);
    }

    if let Some(last_seen_id) = query.last_seen_id {
        qb.push(" AND p.id < ");
        qb.push_bind(last_seen_id);
    }

    if let Some(search) = &query.search {
        qb.push(" AND (p.title ILIKE ");
        qb.push_bind(format!("%{}%", search));
        qb.push(" OR p.description ILIKE ");
        qb.push_bind(format!("%{}%", search));
        qb.push(")");
    }

    qb.push(" GROUP BY p.id ORDER BY p.id DESC LIMIT ");
    qb.push_bind(limit);

    let rows = qb
        .build_query_as::<Product>()
        .fetch_all(pool.get_ref())
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    Ok(HttpResponse::Ok().json(rows))
}

#[derive(Serialize)]
pub struct OptionValue {
    pub value: String,
    pub label: String,
}

#[get("/options/colors")]
async fn get_colors() -> impl Responder {
    let data = vec![
        OptionValue {
            value: "red".into(),
            label: "Червоний".into(),
        },
        OptionValue {
            value: "pink".into(),
            label: "Рожевий".into(),
        },
        OptionValue {
            value: "blue".into(),
            label: "Синій".into(),
        },
        OptionValue {
            value: "yellow".into(),
            label: "Жовтий".into(),
        },
        OptionValue {
            value: "grey".into(),
            label: "Сірий".into(),
        },
        OptionValue {
            value: "black".into(),
            label: "Чорний".into(),
        },
        OptionValue {
            value: "white".into(),
            label: "Білий".into(),
        },
        OptionValue {
            value: "other".into(),
            label: "Інший".into(),
        },
    ];
    HttpResponse::Ok().json(data)
}

#[get("/options/shoe-sizes")]
async fn get_shoe_sizes() -> impl Responder {
    let data = vec![
        OptionValue {
            value: "38".into(),
            label: "24".parse().unwrap(),
        },
        OptionValue {
            value: "38".into(),
            label: "25".parse().unwrap(),
        },
        OptionValue {
            value: "38".into(),
            label: "26".parse().unwrap(),
        },
        OptionValue {
            value: "38".into(),
            label: "27".parse().unwrap(),
        },
        OptionValue {
            value: "38".into(),
            label: "28".parse().unwrap(),
        },
        OptionValue {
            value: "38".into(),
            label: "29".parse().unwrap(),
        },
        OptionValue {
            value: "38".into(),
            label: "30".parse().unwrap(),
        },
        OptionValue {
            value: "38".into(),
            label: "31".parse().unwrap(),
        },
        OptionValue {
            value: "38".into(),
            label: "32".parse().unwrap(),
        },
        OptionValue {
            value: "38".into(),
            label: "33".parse().unwrap(),
        },
        OptionValue {
            value: "38".into(),
            label: "34".parse().unwrap(),
        },
        OptionValue {
            value: "38".into(),
            label: "35".parse().unwrap(),
        },
        OptionValue {
            value: "38".into(),
            label: "36".parse().unwrap(),
        },
        OptionValue {
            value: "38".into(),
            label: "37".parse().unwrap(),
        },
        OptionValue {
            value: "38".into(),
            label: "38".parse().unwrap(),
        },
        OptionValue {
            value: "38".into(),
            label: "39".parse().unwrap(),
        },
        OptionValue {
            value: "40".into(),
            label: "40".parse().unwrap(),
        },
        OptionValue {
            value: "40".into(),
            label: "41".parse().unwrap(),
        },
        OptionValue {
            value: "40".into(),
            label: "42".parse().unwrap(),
        },
        OptionValue {
            value: "40".into(),
            label: "43".parse().unwrap(),
        },
        OptionValue {
            value: "40".into(),
            label: "44".parse().unwrap(),
        },
        OptionValue {
            value: "40".into(),
            label: "45".parse().unwrap(),
        },
        OptionValue {
            value: "40".into(),
            label: "46".parse().unwrap(),
        },
    ];
    HttpResponse::Ok().json(data)
}

#[get("/options/clothing-sizes")]
async fn get_clothing_sizes() -> impl Responder {
    let data = vec![
        OptionValue {
            value: "S".into(),
            label: "Small".parse().unwrap(),
        },
        OptionValue {
            value: "M".into(),
            label: "Medium".parse().unwrap(),
        },
        OptionValue {
            value: "L".into(),
            label: "Large".parse().unwrap(),
        },
        OptionValue {
            value: "XL".into(),
            label: "XLarge".parse().unwrap(),
        },
        OptionValue {
            value: "XXL".into(),
            label: "XXLarge".parse().unwrap(),
        },
        OptionValue {
            value: "XXXL".into(),
            label: "XXXLarge".parse().unwrap(),
        },
        OptionValue {
            value: "XXXXL".into(),
            label: "XXXLarge".parse().unwrap(),
        },
    ];
    HttpResponse::Ok().json(data)
}

#[get("/options/genders")]
async fn get_genders() -> impl Responder {
    let data = vec![
        OptionValue {
            value: "male".into(),
            label: "Чоловіче".into(),
        },
        OptionValue {
            value: "female".into(),
            label: "Жіноче".into(),
        },
        OptionValue {
            value: "kids".into(),
            label: "Дитяче".into(),
        },
        OptionValue {
            value: "unisex".into(),
            label: "Унісекс".into(),
        },
    ];
    HttpResponse::Ok().json(data)
}

#[derive(Serialize)]
pub struct ProductCharacteristics {
    pub shoe_materials: Vec<OptionValue>,
    pub clothing_materials: Vec<OptionValue>,
    pub home_types: Vec<OptionValue>,
    pub home_materials: Vec<OptionValue>,
    pub book_genres: Vec<OptionValue>,
    pub book_binding: Vec<OptionValue>,
    pub book_languages: Vec<OptionValue>,
    pub garden_types: Vec<OptionValue>,
    pub electronics_types: Vec<OptionValue>,
    pub auto_types: Vec<OptionValue>,
    pub stationery_types: Vec<OptionValue>,
    pub activity_types: Vec<OptionValue>,
    pub tourism_types: Vec<OptionValue>,
    pub water_sports_types: Vec<OptionValue>,
    pub cycling_types: Vec<OptionValue>,
    pub climbing_types: Vec<OptionValue>,
    pub picnic_types: Vec<OptionValue>,
    pub children_types: Vec<OptionValue>,
}

#[get("/options/materials")]
async fn get_materials() -> impl Responder {
    let data = ProductCharacteristics {
        shoe_materials: vec![
            OptionValue {
                value: "suede".into(),
                label: "Замша".into(),
            },
            OptionValue {
                value: "nubuck".into(),
                label: "Нубук".into(),
            },
            OptionValue {
                value: "mesh".into(),
                label: "Сітка".into(),
            },
            OptionValue {
                value: "other_shoes".into(),
                label: "Інший".into(),
            },
        ],
        clothing_materials: vec![
            OptionValue {
                value: "cotton".into(),
                label: "Бавовна".into(),
            },
            OptionValue {
                value: "wool".into(),
                label: "Вовна".into(),
            },
            OptionValue {
                value: "linen".into(),
                label: "Льон".into(),
            },
            OptionValue {
                value: "silk".into(),
                label: "Шовк".into(),
            },
            OptionValue {
                value: "polyester".into(),
                label: "Поліестер".into(),
            },
            OptionValue {
                value: "nylon".into(),
                label: "Нейлон".into(),
            },
            OptionValue {
                value: "acrylic".into(),
                label: "Акрил".into(),
            },
            OptionValue {
                value: "viscose".into(),
                label: "Віскоза".into(),
            },
            OptionValue {
                value: "denim".into(),
                label: "Джинс".into(),
            },
            OptionValue {
                value: "other_clothes".into(),
                label: "Інший".into(),
            },
        ],
        home_types: vec![
            OptionValue {
                value: "dishes".into(),
                label: "Посуд".into(),
            },
            OptionValue {
                value: "textile".into(),
                label: "Текстиль".into(),
            },
            OptionValue {
                value: "furniture".into(),
                label: "Меблі".into(),
            },
            OptionValue {
                value: "decor".into(),
                label: "Декор".into(),
            },
            OptionValue {
                value: "lighting".into(),
                label: "Освітлення".into(),
            },
            OptionValue {
                value: "other".into(),
                label: "Інший".into(),
            },
        ],
        home_materials: vec![
            OptionValue {
                value: "wood".into(),
                label: "Дерево".into(),
            },
            OptionValue {
                value: "glass".into(),
                label: "Скло".into(),
            },
            OptionValue {
                value: "ceramic".into(),
                label: "Кераміка".into(),
            },
            OptionValue {
                value: "metal".into(),
                label: "Метал".into(),
            },
            OptionValue {
                value: "fabric".into(),
                label: "Тканина".into(),
            },
            OptionValue {
                value: "plastic".into(),
                label: "Пластик".into(),
            },
            OptionValue {
                value: "other".into(),
                label: "Інше".into(),
            },
        ],
        book_genres: vec![
            OptionValue {
                value: "fiction".into(),
                label: "Художня література".into(),
            },
            OptionValue {
                value: "non_fiction".into(),
                label: "Нехудожня література".into(),
            },
            OptionValue {
                value: "children".into(),
                label: "Дитяча література".into(),
            },
            OptionValue {
                value: "self_development".into(),
                label: "Саморозвиток".into(),
            },
            OptionValue {
                value: "business".into(),
                label: "Бізнес".into(),
            },
            OptionValue {
                value: "history".into(),
                label: "Історія".into(),
            },
            OptionValue {
                value: "fantasy".into(),
                label: "Фантастика".into(),
            },
            OptionValue {
                value: "detective".into(),
                label: "Детектив".into(),
            },
            OptionValue {
                value: "comics".into(),
                label: "Комікс".into(),
            },
            OptionValue {
                value: "novel".into(),
                label: "Роман".into(),
            },
        ],
        book_binding: vec![
            OptionValue {
                value: "soft".into(),
                label: "М'яка".into(),
            },
            OptionValue {
                value: "hard".into(),
                label: "Тверда".into(),
            },
        ],
        book_languages: vec![
            OptionValue {
                value: "ukrainian".into(),
                label: "Українська".into(),
            },
            OptionValue {
                value: "english".into(),
                label: "Англійська".into(),
            },
            OptionValue {
                value: "german".into(),
                label: "Німецька".into(),
            },
            OptionValue {
                value: "other".into(),
                label: "Інше".into(),
            },
        ],
        garden_types: vec![
            OptionValue {
                value: "tools".into(),
                label: "Інвентар (лопата, граблі, сапка, лійка, секатор)".into(),
            },
            OptionValue {
                value: "equipment".into(),
                label: "Техніка (газонокосарка, оприскувач)".into(),
            },
            OptionValue {
                value: "seeds".into(),
                label: "Насіння (овочі, квіти, фрукти)".into(),
            },
            OptionValue {
                value: "fertilizers".into(),
                label: "Добрива (проти шкідників, для росту)".into(),
            },
            OptionValue {
                value: "containers".into(),
                label: "Ємності (горщик, кашпо, контейнер для розсади, ящик, каністра, відро)".into(),
            },
            OptionValue {
                value: "furniture".into(),
                label: "Меблі для саду (стільці, лавки, дивани, столи, набори)".into(),
            },
            OptionValue {
                value: "decor".into(),
                label: "Декор (статуетки, фонтани, камені, плитка)".into(),
            },
            OptionValue {
                value: "lighting".into(),
                label: "Освітлення (сонячна лампа, ліхтар, гірлянда)".into(),
            },
            OptionValue {
                value: "fencing".into(),
                label: "Огорожі (пластикові, дерев'яні, металічні)".into(),
            },
            OptionValue {
                value: "other".into(),
                label: "Інший".into(),
            },
        ],
        electronics_types: vec![
            OptionValue {
                value: "phone".into(),
                label: "Телефон".into(),
            },
            OptionValue {
                value: "laptop".into(),
                label: "Ноутбук".into(),
            },
            OptionValue {
                value: "tablet".into(),
                label: "Планшет".into(),
            },
            OptionValue {
                value: "headphones".into(),
                label: "Навушники".into(),
            },
            OptionValue {
                value: "watch".into(),
                label: "Годинник".into(),
            },
            OptionValue {
                value: "camera".into(),
                label: "Фотоапарат".into(),
            },
            OptionValue {
                value: "tv".into(),
                label: "Телевізор".into(),
            },
            OptionValue {
                value: "fridge".into(),
                label: "Холодильник".into(),
            },
            OptionValue {
                value: "dishwasher".into(),
                label: "Посудомийка".into(),
            },
            OptionValue {
                value: "game_console".into(),
                label: "Приставка".into(),
            },
            OptionValue {
                value: "washing_machine".into(),
                label: "Пральна машина".into(),
            },
            OptionValue {
                value: "speakers".into(),
                label: "Колонки".into(),
            },
            OptionValue {
                value: "sewing_machine".into(),
                label: "Швейна машинка".into(),
            },
            OptionValue {
                value: "other".into(),
                label: "Інший".into(),
            },
        ],
        auto_types: vec![
            OptionValue {
                value: "accessories".into(),
                label: "Аксесуари".into(),
            },
            OptionValue {
                value: "parts".into(),
                label: "Запчастини".into(),
            },
            OptionValue {
                value: "electronics".into(),
                label: "Автоелектроніка".into(),
            },
            OptionValue {
                value: "fluids".into(),
                label: "Масло та рідини".into(),
            },
            OptionValue {
                value: "care".into(),
                label: "Догляд".into(),
            },
            OptionValue {
                value: "tires".into(),
                label: "Шини".into(),
            },
            OptionValue {
                value: "rims".into(),
                label: "Диски".into(),
            },
            OptionValue {
                value: "other".into(),
                label: "Інший".into(),
            },
        ],
        stationery_types: vec![
            OptionValue {
                value: "writing".into(),
                label: "Пишучі прилади (гелеві ручки, кулькові ручки, механічні олівці, графітні олівці, кольорові олівці, маркери)".into(),
            },
            OptionValue {
                value: "paper".into(),
                label: "Паперова продукція (зошит в клітинку, зошит в лінійку, щоденник, блокнот, калька, стікери для нотаток, папір для друку, картон/ватман)".into(),
            },
            OptionValue {
                value: "organization".into(),
                label: "Організація документів (папки, файли, розділювачі, обкладинки, підставки для ручок, органайзери)".into(),
            },
            OptionValue {
                value: "office".into(),
                label: "Офісне приладдя (степлер, скоби, скрепки, кнопки, клей-олівець, ножиці, лінійка, калькулятор)".into(),
            },
            OptionValue {
                value: "art".into(),
                label: "Творчість (альбом для малювання, фарби, художні кисті, фломастери, пластилін, крейда, наліпки, клей)".into(),
            },
            OptionValue {
                value: "other".into(),
                label: "Інший".into(),
            },
        ],
        activity_types: vec![
            OptionValue {
                value: "tourism".into(),
                label: "Туризм та походи".into(),
            },
            OptionValue {
                value: "water_sports".into(),
                label: "Водні види спорту".into(),
            },
            OptionValue {
                value: "cycling".into(),
                label: "Велоспорт".into(),
            },
            OptionValue {
                value: "climbing".into(),
                label: "Альпінізм".into(),
            },
            OptionValue {
                value: "picnic".into(),
                label: "Пікнік".into(),
            },
            OptionValue {
                value: "other".into(),
                label: "Інший".into(),
            },
        ],
        tourism_types: vec![
            OptionValue {
                value: "tent".into(),
                label: "Намет".into(),
            },
            OptionValue {
                value: "sleeping_bag".into(),
                label: "Спальний мішок".into(),
            },
            OptionValue {
                value: "burner".into(),
                label: "Пальник".into(),
            },
            OptionValue {
                value: "backpack".into(),
                label: "Рюкзак".into(),
            },
            OptionValue {
                value: "sleeping_pad".into(),
                label: "Каремат".into(),
            },
            OptionValue {
                value: "dishes".into(),
                label: "Посуд".into(),
            },
            OptionValue {
                value: "compass".into(),
                label: "Компас".into(),
            },
            OptionValue {
                value: "other".into(),
                label: "Інший".into(),
            },
        ],
        water_sports_types: vec![
            OptionValue {
                value: "goggles".into(),
                label: "Водні окуляри та маски".into(),
            },
            OptionValue {
                value: "fins".into(),
                label: "Ласти".into(),
            },
            OptionValue {
                value: "boards".into(),
                label: "Дошки".into(),
            },
            OptionValue {
                value: "paddles".into(),
                label: "Весла".into(),
            },
            OptionValue {
                value: "life_jackets".into(),
                label: "Рятувальні жилети".into(),
            },
            OptionValue {
                value: "kayaks".into(),
                label: "Байдарки".into(),
            },
            OptionValue {
                value: "pump".into(),
                label: "Насос".into(),
            },
            OptionValue {
                value: "other".into(),
                label: "Інший".into(),
            },
        ],
        cycling_types: vec![
            OptionValue {
                value: "bicycle".into(),
                label: "Велосипед".into(),
            },
            OptionValue {
                value: "wheels".into(),
                label: "Колеса".into(),
            },
            OptionValue {
                value: "pump".into(),
                label: "Насос".into(),
            },
            OptionValue {
                value: "helmet".into(),
                label: "Шолом".into(),
            },
            OptionValue {
                value: "lights".into(),
                label: "Ліхтарі".into(),
            },
            OptionValue {
                value: "other".into(),
                label: "Інший".into(),
            },
        ],
        climbing_types: vec![
            OptionValue {
                value: "climbing_shoes".into(),
                label: "Скельники".into(),
            },
            OptionValue {
                value: "protection".into(),
                label: "Страхування".into(),
            },
            OptionValue {
                value: "carabiner".into(),
                label: "Карабін".into(),
            },
            OptionValue {
                value: "rope".into(),
                label: "Мотузка".into(),
            },
            OptionValue {
                value: "helmet".into(),
                label: "Каска".into(),
            },
            OptionValue {
                value: "other".into(),
                label: "Інший".into(),
            },
        ],
        picnic_types: vec![
            OptionValue {
                value: "plaid".into(),
                label: "Плед".into(),
            },
            OptionValue {
                value: "dishes".into(),
                label: "Посуд".into(),
            },
            OptionValue {
                value: "burner".into(),
                label: "Пальник".into(),
            },
            OptionValue {
                value: "other".into(),
                label: "Інший".into(),
            },
        ],
        children_types: vec![
            OptionValue {
                value: "clothes".into(),
                label: "Одяг (комбінезон, футболки, штани, боді, піжама)".into(),
            },
            OptionValue {
                value: "shoes".into(),
                label: "Взуття (повсякденне, зимове, гумові чоботи, інше)".into(),
            },
            OptionValue {
                value: "toys".into(),
                label: "Іграшки (м'які, розвиваючі, конструктори, для вулиці, інтерактивні, інші)".into(),
            },
            OptionValue {
                value: "care".into(),
                label: "Догляд (підгузки, ванночки, термометри, шампуні, щітки, інші)".into(),
            },
            OptionValue {
                value: "education".into(),
                label: "Навчання та творчість (розмальовка, для ліплення, пазли, навчальні зошити, абетка, цифри, інше)".into(),
            },
            OptionValue {
                value: "other".into(),
                label: "Інший".into(),
            },
        ],
    };

    HttpResponse::Ok().json(data)
}
