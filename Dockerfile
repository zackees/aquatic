# Run aquatic_ws in Docker (place in aquatic_ws/Dockerfile)
#
# $ docker build -t aquatic-ws -f aquatic_ws/Dockerfile .
# $ docker run -it --rm -p 3000:3000 --name aquatic-ws aquatic-ws

FROM rust:latest
WORKDIR /usr/src/aquatic
COPY . .
RUN . ./scripts/env-native-cpu-without-avx-512 && cargo install --path ./aquatic_ws
RUN ./scripts/gen-tls.sh
RUN echo "log_level = 'debug'\n[network]\naddress = '0.0.0.0:3000'\ntls_certificate_path = './tmp/tls/cert.crt'\ntls_private_key_path = './tmp/tls/key.pk8'" > ./ws.toml
EXPOSE 3000
CMD ["aquatic_ws", "-c", "./ws.toml"]