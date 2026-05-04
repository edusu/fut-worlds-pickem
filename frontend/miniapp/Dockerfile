# syntax=docker/dockerfile:1.6
# Multi-stage build for the Mini App. The output is a static SPA served by nginx.

FROM node:24-alpine AS builder
WORKDIR /app
RUN corepack enable

# Cache dependencies separately from source.
COPY package.json pnpm-lock.yaml* ./
RUN pnpm install --frozen-lockfile=false

COPY . .
RUN pnpm build

FROM nginx:alpine AS runtime
# SPA fallback so client-side routing works.
RUN printf 'server { \n\
    listen 80; \n\
    root /usr/share/nginx/html; \n\
    index index.html; \n\
    location / { try_files $uri /index.html; } \n\
}\n' > /etc/nginx/conf.d/default.conf

COPY --from=builder /app/dist /usr/share/nginx/html
EXPOSE 80
