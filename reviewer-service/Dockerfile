FROM python:3.12-slim

ENV PYTHONUNBUFFERED=1 PYTHONDONTWRITEBYTECODE=1
WORKDIR /srv
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt
COPY app ./app
RUN useradd --system --no-create-home reviewer
USER reviewer
EXPOSE 8080
# REVIEWER_TOKENS and ANTHROPIC_API_KEY come from the host's secret store, never the image.
CMD ["uvicorn", "app.main:from_environment", "--factory", "--host", "0.0.0.0", "--port", "8080", "--no-access-log"]
