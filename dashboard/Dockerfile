# ─── Stage 1: Build Vite app ────────────────────────────────────────────────
FROM node:20-alpine AS builder

WORKDIR /app

# Install dependencies first (cached layer)
COPY package.json package-lock.json* ./
RUN npm ci

# Receive VITE_* variables as build args so they are baked into the bundle.
# Override with: docker build --build-arg VITE_API_URL=... .
ARG VITE_API_URL=http://localhost:8080
ARG VITE_GATEWAY_URL=http://localhost:8081

ENV VITE_API_URL=$VITE_API_URL
ENV VITE_GATEWAY_URL=$VITE_GATEWAY_URL

COPY . .
RUN npm run build

# ─── Stage 2: Serve with Nginx ───────────────────────────────────────────────
FROM nginx:1.25-alpine

# SPA routing config — all unmatched paths fall through to index.html
COPY nginx.dev.conf /etc/nginx/conf.d/default.conf

# Copy built assets
COPY --from=builder /app/dist /usr/share/nginx/html

EXPOSE 80
CMD ["nginx", "-g", "daemon off;"]
