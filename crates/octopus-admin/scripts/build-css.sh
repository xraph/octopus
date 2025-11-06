#!/bin/bash
set -e

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}Building Octopus Admin CSS...${NC}"

cd "$(dirname "$0")/.."

# Check if npm is installed
if ! command -v npm &> /dev/null; then
    echo "Error: npm is not installed. Please install Node.js first."
    exit 1
fi

# Install dependencies if needed
if [ ! -d "node_modules" ]; then
    echo -e "${BLUE}Installing npm dependencies...${NC}"
    npm install
fi

# Build CSS
echo -e "${BLUE}Compiling Tailwind CSS...${NC}"
npm run build:css

echo -e "${GREEN}✓ CSS build complete!${NC}"
echo -e "Output: ${BLUE}static/output.css${NC}"

