# The image needs a hash-locked requirements.lock and a base image pinned by digest (README,
# "Deploy"). The build stops without them:
#   docker build --build-arg PYTHON_IMAGE=python:3.12-slim@sha256:<digest> -t silo-reviewer .
ARG PYTHON_IMAGE
FROM ${PYTHON_IMAGE:?pass --build-arg PYTHON_IMAGE=python:3.12-slim@sha256:<digest>}

ENV PYTHONUNBUFFERED=1 PYTHONDONTWRITEBYTECODE=1
WORKDIR /srv
COPY requirements.lock ./
RUN pip install --no-cache-dir --require-hashes -r requirements.lock
COPY app ./app
RUN useradd --system --no-create-home reviewer
USER reviewer
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD ["python", "-c", "import urllib.request,sys; sys.exit(0 if urllib.request.urlopen('http://127.0.0.1:8080/healthz', timeout=4).status == 200 else 1)"]
# REVIEWER_TOKENS and ANTHROPIC_API_KEY come from the host's secret store, never the image.
CMD ["uvicorn", "app.main:from_environment", "--factory", "--host", "0.0.0.0", "--port", "8080", "--no-access-log"]
