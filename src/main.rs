use actix_web::{App, HttpServer, web};
use dotenv;
use sqlx::postgres::PgPoolOptions;
use std::env;

mod handlers;
mod services;

use crate::handlers::auth::{
    SignupRequest, confirm, login, logout, otp_verify, refresh_token, reset_password, signup,
    update_password,
};
use crate::handlers::products::{
    categories as product_categories, create as product_create, delivery_options, get_products,
    payment_options,
};
use crate::handlers::users::{categories as user_categories, create as user_create};
use actix_cors::Cors;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::handlers::auth::signup,
    ),
    components(
        schemas(SignupRequest)
    ),
    tags(
        (name = "Auth", description = "Register users.")
    )
)]
pub struct ApiDoc;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!("Starting server");

    dotenv::from_filename("env").ok();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = PgPoolOptions::new()
        .max_connections(5) // Максимальна кількість з'єднань
        .connect(&database_url)
        .await
        .expect("Failed to create pool.");

    HttpServer::new(move || {
        App::new()
            .wrap(
                Cors::default()
                    .allow_any_origin() // або .allowed_origin("https://твій-домен")
                    .allow_any_method()
                    .allow_any_header(),
            )
            .app_data(web::Data::new(pool.clone()))
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}")
                    .url("/api-doc/openapi.json", ApiDoc::openapi()),
            )
            .service(
                web::scope("/api/v1")
                    .service(
                        web::scope("/auth")
                            .service(signup)
                            .service(confirm)
                            .service(login)
                            .service(logout)
                            .service(refresh_token)
                            .service(reset_password)
                            .service(otp_verify)
                            .service(update_password),
                    )
                    .service(
                        web::scope("/users")
                            .service(user_create)
                            .service(user_categories),
                    )
                    .service(
                        web::scope("/products")
                            .service(product_categories)
                            .service(payment_options)
                            .service(delivery_options)
                            .service(product_create)
                            .service(get_products),
                    ),
            )
    })
    .bind(("0.0.0.0", 4000))?
    .run()
    .await
}
