use actix_web::{HttpResponse, Responder, get, post, web};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub enum ChatStatus {
    Active,
    Inactive,
    Request,
}

#[derive(Debug, ToSchema, Serialize, Deserialize)]
pub struct ChatResponse {
    id: i32,
    status: ChatStatus,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
}

#[derive(Debug, ToSchema, Serialize, Deserialize)]
pub struct MessageResponse {
    id: i32,
    chat_id: i32,
    sender_id: i32,
    message: String,
    is_read: bool,
    sent_at: NaiveDateTime,
}

#[derive(Debug, ToSchema, Serialize, Deserialize)]
pub struct UnreadCountResponse {
    count: i32,
}

#[derive(Debug, ToSchema, Serialize, Deserialize)]
pub struct CreateChatRequest {
    requestor_id: i32,
    recipient_id: i32,
}

#[derive(Debug, ToSchema, Serialize, Deserialize)]
pub struct CreateMessageRequest {
    chat_id: i32,
}

#[utoipa::path(
    tag = "chat",
    responses(
        (status = 200, description = "List of all chats", body = Vec<ChatResponse>),
    )
)]
#[get("")]
pub async fn chat_list(_db_pool: web::Data<PgPool>) -> Result<impl Responder, actix_web::Error> {
    // TODO
    Ok(HttpResponse::Ok())
}

#[utoipa::path(
    tag = "chat",
    responses(
        (status = 200, description = "Chat created", body = ChatResponse),
        (status = 409, description = "Chat already exists")
    ),
    request_body = CreateChatRequest
)]
#[post("")]
pub async fn chat_create(_db_pool: web::Data<PgPool>) -> Result<impl Responder, actix_web::Error> {
    // TODO
    Ok(HttpResponse::Ok())
}

#[utoipa::path(
    tag = "chat",
    responses(
        (status = 200, description = "List of all messages in the chat", body = Vec<MessageResponse>),
    ),
    params(
        ("chat_id" = i32, Path),
    )
)]
#[get("/{chat_id}")]
pub async fn message_list(_db_pool: web::Data<PgPool>) -> Result<impl Responder, actix_web::Error> {
    // TODO
    Ok(HttpResponse::Ok())
}

#[utoipa::path(
    tag = "chat",
    responses(
        (status = 200, description = "Message created", body = MessageResponse),
    ),
    request_body = CreateMessageRequest
)]
#[post("")]
pub async fn message_create(
    _db_pool: web::Data<PgPool>,
) -> Result<impl Responder, actix_web::Error> {
    // TODO
    Ok(HttpResponse::Ok())
}

#[utoipa::path(
    tag = "chat",
    responses(
        (status = 200, description = "Count of unread messages in the chat for the user", body = UnreadCountResponse),
    ),
    params(
        ("chat_id" = i32, Path),
    )
)]
#[get("unread_count/{chat_id}")]
pub async fn message_unread_count(
    _db_pool: web::Data<PgPool>,
) -> Result<impl Responder, actix_web::Error> {
    // TODO
    Ok(HttpResponse::Ok())
}

#[utoipa::path(
    tag = "chat",
    responses(
        (status = 200, description = "Messages are marked as read"),
    ),
    params(
        ("chat_id" = i32, Path),
    )
)]
#[get("mark_read/{chat_id}")]
pub async fn message_mark_read(
    _db_pool: web::Data<PgPool>,
) -> Result<impl Responder, actix_web::Error> {
    // TODO
    Ok(HttpResponse::Ok())
}
