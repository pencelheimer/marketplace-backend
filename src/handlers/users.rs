use crate::handlers::auth::AuthenticatedUser;
use actix_web::{HttpResponse, Responder, post, web};
use serde::Deserialize;
use sqlx::{PgPool, Postgres, QueryBuilder};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct CreateRequest {
    is_buyer: bool,
    is_seller: bool,
}

async fn update_user_role(
    db_pool: &PgPool,
    user_id: &Uuid,
    table: &str,
) -> Result<(), actix_web::Error> {
    sqlx::query(&format!("DELETE FROM {} WHERE user_id = $1", table))
        .bind(user_id)
        .execute(db_pool)
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    let result = sqlx::query(&format!("INSERT INTO {} (user_id) VALUES ($1)", table))
        .bind(user_id)
        .execute(db_pool)
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    if result.rows_affected() == 0 {
        return Err(actix_web::error::ErrorInternalServerError(
            "Failed to insert role",
        ));
    }

    Ok(())
}

#[post("/create")]
async fn create(
    user: AuthenticatedUser,
    req: web::Json<CreateRequest>,
    db_pool: web::Data<PgPool>,
) -> Result<impl Responder, actix_web::Error> {
    let user_id = &user.0.sub;

    if req.is_buyer {
        update_user_role(db_pool.get_ref(), user_id, "buyers").await?;
    }

    if req.is_seller {
        update_user_role(db_pool.get_ref(), user_id, "sellers").await?;
    }

    Ok(HttpResponse::Ok().body("User roles updated successfully"))
}

#[derive(Deserialize)]
pub struct CategoryRequest {
    category_id: i32,
}

#[derive(Deserialize)]
pub struct CategoriesRequest {
    categories: Vec<CategoryRequest>,
}

#[post("/categories")]
async fn categories(
    user: AuthenticatedUser,
    req: web::Json<CategoriesRequest>,
    db_pool: web::Data<PgPool>,
) -> Result<impl Responder, actix_web::Error> {
    let user_id = &user.0.sub;

    sqlx::query("DELETE FROM user_categories WHERE user_id = $1")
        .bind(user_id)
        .execute(db_pool.get_ref())
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    if req.categories.is_empty() {
        return Ok(HttpResponse::Ok().body("User categories cleared"));
    }

    let mut builder: QueryBuilder<Postgres> =
        QueryBuilder::new("INSERT INTO user_categories (user_id, category_id) ");

    builder.push_values(&req.categories, |mut b, cat| {
        b.push_bind(user_id).push_bind(cat.category_id);
    });

    builder
        .build()
        .execute(db_pool.get_ref())
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    Ok(HttpResponse::Ok().body("User categories updated successfully"))
}
