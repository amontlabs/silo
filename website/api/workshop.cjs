// Vercel Node.js entry point for every /api/workshop/* function.
const loading = import('../server/node.mjs');

module.exports = async (req, res) => (await loading).vercelHandler(req, res);
