#!/bin/bash
# Extremal GCP Setup Reference Script
# Run these commands step by step — don't execute this file blindly.
# Replace placeholders: PROJECT_ID, DB_PASSWORD, REGION

set -euo pipefail

PROJECT_ID="your-project-id"
REGION="us-central1"
DB_PASSWORD="change-me"

echo "=== Step 1: Enable APIs ==="
gcloud config set project "$PROJECT_ID"
gcloud services enable \
  run.googleapis.com \
  sqladmin.googleapis.com \
  cloudbuild.googleapis.com \
  secretmanager.googleapis.com \
  artifactregistry.googleapis.com

echo "=== Step 2: Artifact Registry ==="
gcloud artifacts repositories create extremal \
  --repository-format=docker \
  --location="$REGION" \
  || echo "Already exists"

echo "=== Step 3: Cloud SQL ==="
gcloud sql instances create extremal \
  --database-version=POSTGRES_16 \
  --tier=db-f1-micro \
  --region="$REGION" \
  --root-password="$DB_PASSWORD"

gcloud sql databases create extremal --instance=extremal
gcloud sql users create extremal --instance=extremal --password="$DB_PASSWORD"

echo "=== Step 4: Server Key ==="
echo "Generate key locally first:"
echo "  cargo run -p extremal-cli -- keygen --name extremal-server -o server-key.json"
echo "Then upload:"
echo "  gcloud secrets create extremal-server-key --data-file=server-key.json"
echo "  rm server-key.json"

echo "=== Step 5: Cloud Build Permissions ==="
PROJECT_NUM=$(gcloud projects describe "$PROJECT_ID" --format='value(projectNumber)')

gcloud projects add-iam-policy-binding "$PROJECT_ID" \
  --member="serviceAccount:${PROJECT_NUM}@cloudbuild.gserviceaccount.com" \
  --role="roles/run.admin"

gcloud iam service-accounts add-iam-policy-binding \
  "${PROJECT_NUM}-compute@developer.gserviceaccount.com" \
  --member="serviceAccount:${PROJECT_NUM}@cloudbuild.gserviceaccount.com" \
  --role="roles/iam.serviceAccountUser"

echo "=== Step 6: Build & Deploy Server ==="
gcloud builds submit \
  --tag "$REGION-docker.pkg.dev/$PROJECT_ID/extremal/server" \
  --timeout=600s

gcloud run deploy extremal-server \
  --image "$REGION-docker.pkg.dev/$PROJECT_ID/extremal/server" \
  --region "$REGION" --allow-unauthenticated \
  --add-cloudsql-instances "$PROJECT_ID:$REGION:extremal" \
  --set-env-vars "\
DATABASE_URL=postgres://extremal:${DB_PASSWORD}@/extremal?host=/cloudsql/${PROJECT_ID}:${REGION}:extremal,\
ALLOWED_ORIGINS=https://extremal.online,\
DB_MAX_CONNECTIONS=5,LEADERBOARD_CAPACITY=500,MAX_N=62" \
  --set-secrets "SERVER_KEY_PATH=/secrets/key.json:extremal-server-key:latest" \
  --min-instances 1 --max-instances 4 \
  --cpu 2 --memory 1Gi --concurrency 50 --timeout 60s \
  --port 3001 --command extremal-server --args "--migrate"

echo "=== Step 7: Build & Deploy Web App ==="
gcloud builds submit \
  --tag "$REGION-docker.pkg.dev/$PROJECT_ID/extremal/web" \
  -f Dockerfile.web --timeout=300s

gcloud run deploy extremal-web \
  --image "$REGION-docker.pkg.dev/$PROJECT_ID/extremal/web" \
  --region "$REGION" --allow-unauthenticated \
  --set-env-vars "API_URL=https://api.extremal.online" \
  --min-instances 0 --max-instances 2 \
  --cpu 1 --memory 256Mi --concurrency 100 --timeout 30s \
  --port 8080

echo "=== Step 8: Build & Deploy Dashboard ==="
gcloud builds submit \
  --tag "$REGION-docker.pkg.dev/$PROJECT_ID/extremal/dashboard" \
  -f Dockerfile.dashboard --timeout=600s

gcloud run deploy extremal-dashboard \
  --image "$REGION-docker.pkg.dev/$PROJECT_ID/extremal/dashboard" \
  --region "$REGION" --allow-unauthenticated \
  --min-instances 1 --max-instances 1 \
  --cpu 1 --memory 512Mi --concurrency 200 \
  --timeout 3600s --port 4000

echo "=== Step 9: Custom Domains ==="
gcloud run domain-mappings create --service=extremal-server \
  --domain=api.extremal.online --region="$REGION"
gcloud run domain-mappings create --service=extremal-web \
  --domain=extremal.online --region="$REGION"
gcloud run domain-mappings create --service=extremal-dashboard \
  --domain=dashboard.extremal.online --region="$REGION"

echo ""
echo "=== DNS Configuration (do this in Wix) ==="
echo "Add these DNS records in Wix domain management:"
echo ""
echo "  CNAME  api        -> ghs.googlehosted.com."
echo "  CNAME  dashboard  -> ghs.googlehosted.com."
echo "  A      @          -> (get IPs from: gcloud run domain-mappings describe --domain=extremal.online --region=$REGION)"
echo ""
echo "HTTPS certificates are provisioned automatically by Cloud Run after DNS propagation."
