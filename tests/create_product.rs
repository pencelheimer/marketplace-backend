use reqwest::multipart;

#[actix_web::test]
async fn test_create_product_real_url() {
    let file_bytes = std::fs::read("./tests/assets/test.jpg").unwrap();

    let part = multipart::Part::bytes(file_bytes)
        .file_name("test.jpg")
        .mime_str("image/jpeg")
        .unwrap();

    let form = multipart::Form::new()
        .text("title", "New Product")
        .text("description", "Awesome item")
        .text("price", "99.99")
        .text("phone_number", "+380501234567")
        .text("category", "1")
        .text("condition", "new")
        .text("delivery_option", "1,2")
        .text("payment_option", "1")
        .part("photo", part);

    let client = reqwest::Client::new();

    let resp = client
        .post("http://localhost:4000/api/v1/products/create")
        .bearer_auth("eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJzdWIiOiI3MTA4MzRjOS0zMzdkLTRkODItOTdlZi04YzZiNDNkNmMzMWIiLCJlbWFpbCI6ImFsZXhhbmRydmlydHVhbEBnbWFpbC5jb20iLCJleHAiOjE3NDU5NTA3MjV9.oiqrPZj11fa37BTb60xrIFxBAH3boDo-Mg0Wn8cqC8I")
        .multipart(form)
        .send()
        .await;

    match resp {
        Ok(response) => {
            let status = response.status(); // збережемо статус перед переміщенням
            println!("Response Status: {}", status);
            let body = response.text().await.unwrap();
            println!("Response Body: {}", body);

            // Тепер можна використовувати статус без помилки
            assert_eq!(status, 200);
        }
        Err(e) => {
            eprintln!("Request failed: {}", e);
        }
    }
}
