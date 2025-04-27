watch:
	cargo watch -x run

login:
	docker login

build:
	docker buildx build --platform linux/amd64 -t alexandrvirtual/marketplace-api:latest --push .

pull:
	docker pull alexandrvirtual/marketplace-api:latest

run:
	docker run --env-file=env -d -it -p 8032:4000 --name marketplace-api alexandrvirtual/marketplace-api:latest
