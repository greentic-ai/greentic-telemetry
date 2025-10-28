dev-logs-elastic:
	docker compose -f dev/docker-compose.elastic.yml up -d
	@echo "Kibana → http://localhost:5601 (Discover → search your dev logs)"

dev-logs-elastic-down:
	docker compose -f dev/docker-compose.elastic.yml down -v
