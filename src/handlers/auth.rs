use actix_web::error::ErrorUnauthorized;
use actix_web::{Error, FromRequest, HttpRequest, HttpResponse, Responder, get, patch, post, web};
use argon2::password_hash::{PasswordHash, PasswordVerifier, SaltString, rand_core::OsRng};
use argon2::{Argon2, PasswordHasher};
use chrono::Utc;
use futures_util::future::{Ready, ready};
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, TokenData, Validation, decode, encode,
};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Row};
use std::env;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
pub struct SignupRequest {
    first_name: String,
    last_name: String,
    email: String,
    password: String,
}

#[derive(Serialize, ToSchema)]
pub struct SignupResponse {
    message: String,
    token: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    email: String,
    exp: usize,
}

struct EmailConfig {
    host: String,
    from: String,
    user: String,
    password: String,
}

impl EmailConfig {
    fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            host: env::var("EMAIL_HOST")?,
            from: env::var("EMAIL_FROM")?,
            user: env::var("EMAIL_USER")?,
            password: env::var("EMAIL_PASSWORD")?,
        })
    }
}

async fn send_confirmation_email(
    user_email: &str,
    body: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = EmailConfig::from_env()?;

    let email = Message::builder()
        .from(config.from.parse()?)
        .to(user_email.parse()?)
        .subject("Confirm your registration")
        .body(body.to_string())?;

    let creds = Credentials::new(config.user, config.password);

    let mailer = SmtpTransport::relay(&config.host)?
        .credentials(creds)
        .build();

    match mailer.send(&email) {
        Ok(_) => {
            println!("Email sent successfully!");
        }
        Err(e) => {
            eprintln!("Failed to send email: {:?}", e);
            return Err(actix_web::error::ErrorInternalServerError("Failed to send email").into());
        }
    }

    Ok(())
}

#[derive(Serialize, Deserialize, Clone, ToSchema)]
pub(super) enum ErrorResponse {
    /// When Todo is not found by search term.
    NotFound(String),
    /// When there is a conflict storing a new todo.
    Conflict(String),
    /// When todo endpoint was called without correct credentials
    Unauthorized(String),
}

#[utoipa::path(
    request_body = SignupRequest,
    responses(
        (status = 201, description = "User created"),
        (status = 409, description = "User already exists", body = ErrorResponse)
    )
)]
#[post("/register")]
pub async fn signup(
    user: web::Json<SignupRequest>,
    db_pool: web::Data<PgPool>,
) -> Result<impl Responder, actix_web::Error> {
    // Тут може бути логіка реєстрації, перевірка у базі, хешування пароля і т.д.
    let existing_user: Option<(String,)> =
        sqlx::query_as("SELECT email FROM users WHERE email = $1")
            .bind(&user.email)
            .fetch_optional(db_pool.get_ref())
            .await
            .unwrap();

    if existing_user.is_some() {
        return Ok(HttpResponse::Conflict().body("User with this email already exists"));
    }

    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(user.password.as_bytes(), &salt)
        .unwrap()
        .to_string();

    let user_row = sqlx::query(
        "INSERT INTO users (first_name, last_name, email, password) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(&user.first_name)
    .bind(&user.last_name)
    .bind(&user.email)
    .bind(&password_hash)
    .fetch_one(db_pool.get_ref())
    .await.map_err(actix_web::error::ErrorInternalServerError)?;

    let expiration = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::days(7))
        .unwrap()
        .timestamp() as usize;

    let user_id: Uuid = user_row.try_get("id").unwrap();

    let claims = Claims {
        sub: user_id,
        email: user.email.clone(),
        exp: expiration,
    };

    let secret = env::var("JWT_SECRET").unwrap_or_else(|_| "secret".into());
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_ref()),
    )
    .unwrap();

    let body = format!(
        "Please confirm your registration by clicking the following link:\n{}/{}",
        env::var("EMAIL_REGISTRATION_URL").unwrap(),
        token
    );

    send_confirmation_email(user.email.as_str(), &body).await?;

    Ok(HttpResponse::Ok().json(SignupResponse {
        message: "Registration successful".into(),
        token,
    }))
}

#[get("/confirm/{token}")]
async fn confirm(token: web::Path<String>, db_pool: web::Data<PgPool>) -> impl Responder {
    let token = token.into_inner();
    let secret = env::var("JWT_SECRET").unwrap_or_else(|_| "secret".into());

    let mut validation = Validation::default();
    validation.leeway = 0;

    let decoding_key = DecodingKey::from_secret(secret.as_bytes());

    let token_data: Result<TokenData<Claims>, jsonwebtoken::errors::Error> =
        decode(&token, &decoding_key, &validation);

    match token_data {
        Ok(token_data) => {
            let email = token_data.claims.email.clone();

            let _ = sqlx::query("UPDATE users SET active = true WHERE email = $1")
                .bind(&email)
                .execute(db_pool.get_ref())
                .await;

            HttpResponse::Ok().body("Email successfully confirmed!")
        }
        Err(error) => HttpResponse::InternalServerError().body(format!("Error: {}", error)),
    }
}

#[derive(Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(serde::Serialize)]
struct LoginResponse {
    token: String,
}

#[post("/login")]
async fn login(
    creds: web::Json<LoginRequest>,
    db_pool: web::Data<PgPool>,
) -> Result<impl Responder, actix_web::Error> {
    let row = sqlx::query("SELECT id, password, email, active FROM users WHERE email = $1")
        .bind(&creds.email)
        .fetch_optional(db_pool.get_ref())
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    if let Some(user) = row {
        let active: bool = user
            .try_get("active")
            .map_err(actix_web::error::ErrorInternalServerError)?;

        if !active {
            return Ok(HttpResponse::Unauthorized().body("Email not confirmed"));
        }

        let user_password: String = user
            .try_get("password")
            .map_err(actix_web::error::ErrorInternalServerError)?;

        let parsed_hash = PasswordHash::new(&user_password)
            .map_err(actix_web::error::ErrorInternalServerError)?;

        let verified = Argon2::default()
            .verify_password(creds.password.as_bytes(), &parsed_hash)
            .is_ok();

        if verified {
            let user_id: Uuid = user
                .try_get("id")
                .map_err(actix_web::error::ErrorInternalServerError)?;

            let secret = env::var("JWT_SECRET").unwrap_or_else(|_| "secret".to_string());

            let exp = Utc::now().timestamp() as usize + 7 * 24 * 60 * 60;

            let claims = Claims {
                sub: user_id,
                email: creds.email.clone(),
                exp,
            };

            let token = encode(
                &Header::default(),
                &claims,
                &EncodingKey::from_secret(secret.as_ref()),
            )
            .map_err(actix_web::error::ErrorInternalServerError)?;

            return Ok(HttpResponse::Ok().json(LoginResponse { token }));
        }
    }

    Ok(HttpResponse::Unauthorized().body("Invalid credentials"))
}

#[post("/logout")]
async fn logout() -> impl Responder {
    HttpResponse::Ok().body("Logged out (token should be removed on client)")
}

#[derive(Deserialize)]
struct RefreshRequest {
    refresh_token: String,
}

#[post("/refresh-token")]
async fn refresh_token(req: web::Json<RefreshRequest>) -> impl Responder {
    let secret = env::var("JWT_SECRET").unwrap_or("secret".into());

    let decoded = decode::<Claims>(
        &req.refresh_token,
        &DecodingKey::from_secret(secret.as_ref()),
        &Validation::default(),
    );

    match decoded {
        Ok(data) => {
            let new_exp = Utc::now().timestamp() as usize + 7 * 24 * 60 * 60;
            let claims = Claims {
                sub: data.claims.sub,
                email: data.claims.email,
                exp: new_exp,
            };

            let new_token = encode(
                &Header::default(),
                &claims,
                &EncodingKey::from_secret(secret.as_ref()),
            )
            .unwrap();

            HttpResponse::Ok().json(json!({ "token": new_token }))
        }
        Err(_) => HttpResponse::Unauthorized().body("Invalid token"),
    }
}

#[derive(Deserialize)]
struct ResetPasswordRequest {
    email: String,
}

#[derive(Serialize)]
struct ResetPasswordResponse {
    otp: String,
}

#[post("/reset-password")]
async fn reset_password(
    req: web::Json<ResetPasswordRequest>,
    db_pool: web::Data<PgPool>,
) -> Result<impl Responder, actix_web::Error> {
    let email = req.email.clone();

    let row = sqlx::query("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(db_pool.get_ref())
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    if let Some(user) = row {
        let user_id: Uuid = user
            .try_get("id")
            .map_err(actix_web::error::ErrorInternalServerError)?;

        let otp = sqlx::query("INSERT INTO otp_tokens (user_id) VALUES ($1) RETURNING otp")
            .bind(user_id)
            .fetch_one(db_pool.get_ref())
            .await
            .map_err(actix_web::error::ErrorInternalServerError)?;

        let otp_token = otp
            .try_get("otp")
            .map_err(actix_web::error::ErrorInternalServerError)?;

        let body = format!(
            "You requested to reset your password.\n\
             Otp: {}\n\n\
             If you did not request this, please ignore this email.",
            otp_token
        );

        send_confirmation_email(&email, &body).await?;

        return Ok(HttpResponse::Ok().json(ResetPasswordResponse { otp: otp_token }));
    }

    Ok(HttpResponse::Unauthorized().body("User not found"))
}

#[derive(Deserialize)]
struct OtpRequest {
    email: String,
    otp: String,
}

#[derive(Serialize)]
struct OtpResponse {
    message: String,
    token: String,
}

#[post("/otp")]
async fn otp_verify(
    req: web::Json<OtpRequest>,
    db_pool: web::Data<PgPool>,
) -> Result<impl Responder, actix_web::Error> {
    let email = req.email.clone();

    let user_row = sqlx::query("SELECT id, first_name, last_name FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(db_pool.get_ref())
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    if let Some(user) = user_row {
        let user_id: Uuid = user
            .try_get("id")
            .map_err(actix_web::error::ErrorInternalServerError)?;

        let otp = req.otp.clone();

        let otp_row = sqlx::query("SELECT user_id FROM otp_tokens WHERE otp = $1 AND user_id = $2 AND expires_at >= NOW()")
            .bind(&otp)
            .bind(&user_id)
            .fetch_optional(db_pool.get_ref())
            .await
            .map_err(actix_web::error::ErrorInternalServerError)?;

        if otp_row.is_some() {
            let expiration = chrono::Utc::now()
                .checked_add_signed(chrono::Duration::days(7))
                .unwrap()
                .timestamp() as usize;

            let claims = Claims {
                sub: user_id,
                email: email.clone(),
                exp: expiration,
            };

            let secret = env::var("JWT_SECRET").unwrap_or_else(|_| "secret".into());

            let token = encode(
                &Header::default(),
                &claims,
                &EncodingKey::from_secret(secret.as_ref()),
            )
            .unwrap();

            return Ok(HttpResponse::Ok().json(OtpResponse {
                message: "Login successful".into(),
                token: token.clone(),
            }));
        }
    }

    Ok(HttpResponse::Unauthorized().body("Invalid credentials"))
}

#[derive(Debug)]
pub struct AuthenticatedUser(pub Claims);

impl FromRequest for AuthenticatedUser {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _: &mut actix_web::dev::Payload) -> Self::Future {
        let auth_header = req.headers().get("Authorization");

        if let Some(header_value) = auth_header {
            if let Ok(auth_str) = header_value.to_str() {
                if auth_str.starts_with("Bearer ") {
                    let token = &auth_str[7..];

                    let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "secret".into());
                    let key = DecodingKey::from_secret(secret.as_bytes());

                    let validation = Validation::new(Algorithm::HS256);
                    return match decode::<Claims>(token, &key, &validation) {
                        Ok(token_data) => ready(Ok(AuthenticatedUser(token_data.claims))),
                        Err(_) => ready(Err(ErrorUnauthorized("Invalid token"))),
                    };
                }
            }
        }

        ready(Err(ErrorUnauthorized("Missing or malformed token")))
    }
}

#[derive(Deserialize)]
pub struct UpdatePasswordRequest {
    pub password: String,
}

#[patch("/update-password")]
async fn update_password(
    user: AuthenticatedUser,
    req: web::Json<UpdatePasswordRequest>,
    db_pool: web::Data<PgPool>,
) -> Result<impl Responder, actix_web::Error> {
    let user_id = &user.0.sub;

    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(req.password.as_bytes(), &salt)
        .unwrap()
        .to_string();

    let update_password = sqlx::query("UPDATE users SET password = $1 WHERE id = $2")
        .bind(password_hash)
        .bind(user_id)
        .execute(db_pool.get_ref())
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    if update_password.rows_affected() == 0 {
        return Ok(HttpResponse::NotFound().body("User not found"));
    }

    Ok(HttpResponse::Ok().body("Password updated successfully"))
}
