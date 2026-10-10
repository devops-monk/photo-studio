#!/usr/bin/env bash
# Deploy the website to the VPS without touching other sites.
#   deploy/deploy-site.sh            sync files (and install the vhost on first run)
#   deploy/deploy-site.sh --tls      also request the Let's Encrypt certificate
set -euo pipefail
HOST="${DEPLOY_HOST:-root@168.231.79.163}"
DOMAIN="photo-studio.devops-monk.com"
ROOT="/var/www/photo-studio"
HERE="$(cd "$(dirname "$0")" && pwd)"
SITE="$HERE/../website"

echo "→ syncing website to $HOST:$ROOT"
ssh "$HOST" "mkdir -p $ROOT"
rsync -az --delete --exclude '.DS_Store' "$SITE/" "$HOST:$ROOT/"
ssh "$HOST" "chown -R www-data:www-data $ROOT"

# Install the vhost only if it isn't there yet; never edit other sites or nginx.conf.
if ! ssh "$HOST" "test -e /etc/nginx/sites-available/$DOMAIN"; then
  echo "→ installing nginx vhost"
  scp "$HERE/nginx/$DOMAIN.conf" "$HOST:/etc/nginx/sites-available/$DOMAIN"
  ssh "$HOST" "ln -sf /etc/nginx/sites-available/$DOMAIN /etc/nginx/sites-enabled/$DOMAIN"
fi

echo "→ testing nginx config"
if ssh "$HOST" "nginx -t"; then
  ssh "$HOST" "systemctl reload nginx"
else
  echo "nginx -t failed; not reloading. Other sites are untouched." >&2
  exit 1
fi

if [[ "${1:-}" == "--tls" ]]; then
  echo "→ requesting certificate"
  ssh "$HOST" "certbot --nginx -d $DOMAIN --non-interactive --agree-tos --redirect -m abhaypratap3537@gmail.com"
fi
echo "✓ https://$DOMAIN"
