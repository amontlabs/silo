# A small, realistic stack, pointed at moto through the logging proxy (never at real AWS).
terraform {
  required_providers {
    aws = { source = "hashicorp/aws", version = "6.74.0" }
  }
}

variable "endpoint" { default = "http://127.0.0.1:4566" }
variable "db_class" { default = "db.t3.micro" }
variable "objects" { default = 5 }
variable "suffix" { default = "" }

provider "aws" {
  region                      = "us-east-1"
  access_key                  = "test"
  secret_key                  = "test"
  skip_credentials_validation = true
  skip_requesting_account_id  = true
  skip_metadata_api_check     = true
  s3_use_path_style           = true
  endpoints {
    ec2      = var.endpoint
    rds      = var.endpoint
    s3       = var.endpoint
    dynamodb = var.endpoint
    iam      = var.endpoint
    lambda   = var.endpoint
    logs     = var.endpoint
    sts      = var.endpoint
  }
}

resource "aws_vpc" "main" {
  cidr_block           = "10.0.0.0/16"
  enable_dns_hostnames = true
  tags                 = { Name = "prod" }
}

resource "aws_subnet" "a" {
  vpc_id            = aws_vpc.main.id
  cidr_block        = "10.0.1.0/24"
  availability_zone = "us-east-1a"
}

resource "aws_subnet" "b" {
  vpc_id            = aws_vpc.main.id
  cidr_block        = "10.0.2.0/24"
  availability_zone = "us-east-1b"
}

resource "aws_security_group" "db" {
  name   = "prod-db"
  vpc_id = aws_vpc.main.id
}

resource "aws_vpc_security_group_ingress_rule" "pg" {
  security_group_id = aws_security_group.db.id
  cidr_ipv4         = "10.0.0.0/16"
  from_port         = 5432
  to_port           = 5432
  ip_protocol       = "tcp"
}

resource "aws_vpc_security_group_egress_rule" "all" {
  security_group_id = aws_security_group.db.id
  cidr_ipv4         = "0.0.0.0/0"
  ip_protocol       = "-1"
}

resource "aws_db_subnet_group" "main" {
  name       = "prod"
  subnet_ids = [aws_subnet.a.id, aws_subnet.b.id]
}

resource "aws_db_instance" "main" {
  identifier             = "prod-db${var.suffix}"
  engine                 = "postgres"
  instance_class         = var.db_class
  allocated_storage      = 20
  username               = "app"
  password               = "not-a-real-password"
  db_subnet_group_name   = aws_db_subnet_group.main.name
  vpc_security_group_ids = [aws_security_group.db.id]
  skip_final_snapshot    = true
}

resource "aws_s3_bucket" "backups" {
  bucket        = "prod-backups"
  force_destroy = true
}

resource "aws_s3_bucket_versioning" "backups" {
  bucket = aws_s3_bucket.backups.id
  versioning_configuration { status = "Enabled" }
}

resource "aws_s3_object" "dump" {
  count   = var.objects
  bucket  = aws_s3_bucket.backups.id
  key     = "db/dump-${count.index}.sql"
  content = "fake dump ${count.index}"
}

resource "aws_dynamodb_table" "orders" {
  name         = "orders${var.suffix}"
  billing_mode = "PAY_PER_REQUEST"
  hash_key     = "id"
  attribute {
    name = "id"
    type = "S"
  }
}

resource "aws_iam_role" "fn" {
  name = "orders-fn"
  assume_role_policy = jsonencode({
    Version   = "2012-10-17"
    Statement = [{ Effect = "Allow", Action = "sts:AssumeRole", Principal = { Service = "lambda.amazonaws.com" } }]
  })
}

resource "aws_iam_policy" "fn" {
  name = "orders-fn-dynamo"
  policy = jsonencode({
    Version   = "2012-10-17"
    Statement = [{ Effect = "Allow", Action = ["dynamodb:GetItem", "dynamodb:PutItem"], Resource = aws_dynamodb_table.orders.arn }]
  })
}

resource "aws_iam_role_policy_attachment" "fn" {
  role       = aws_iam_role.fn.name
  policy_arn = aws_iam_policy.fn.arn
}

resource "aws_cloudwatch_log_group" "fn" {
  name = "/aws/lambda/orders"
}

resource "aws_lambda_function" "orders" {
  function_name = "orders"
  role          = aws_iam_role.fn.arn
  handler       = "index.handler"
  runtime       = "python3.12"
  filename      = "${path.module}/fn.zip"
  depends_on    = [aws_iam_role_policy_attachment.fn, aws_cloudwatch_log_group.fn]
}

resource "aws_internet_gateway" "main" {
  vpc_id = aws_vpc.main.id
}

resource "aws_route_table" "public" {
  vpc_id = aws_vpc.main.id
  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.main.id
  }
}
