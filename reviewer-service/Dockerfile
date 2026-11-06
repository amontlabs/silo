# Build with the base image pinned by digest (see README, "Deploy"):
#   docker build --build-arg PYTHON_IMAGE=python:3.12-slim@sha256:<digest> -t silo-reviewer .
ARG PYTHON_IMAGE
FROM ${PYTHON_IMAGE:?pass --build-arg PYTHON_IMAGE=python:3.12-slim@sha256:<digest>}

ENV PYTHONUNBUFFERED=1 PYTHONDONTWRITEBYTECODE=1
WORKDIR /srv
# requirements.lock (with hashes, see README) wins when it exists.
COPY requirements.tx[t] requirements.loc[k] ./
RUN if [ -f requirements.lock ]; then pip install --no-cache-dir --require-hashes -r requirements.lock; \
    else pip install --no-cache-dir -r requirements.txt; fi
COPY app ./app
RUN useradd --system --no-create-home reviewer
USER reviewer
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD ["python", "-c", "import urllib.request,sys; sys.exit(0 if urllib.request.urlopen('http://127.0.0.1:8080/healthz', timeout=4).status == 200 else 1)"]
# REVIEWER_TOKENS and ANTHROPIC_API_KEY come from the host's secret store, never the image.
CMD ["uvicorn", "app.main:from_environment", "--factory", "--host", "0.0.0.0", "--port", "8080", "--no-access-log"]
