export const user = {
  name: 'Jackson Lee',
  mode: 'Local Mode',
  avatar: 'JL',
};

export const gpu = {
  model: 'NVIDIA RTX 4070 Ti',
  utilization: 42,
  temperature: 48,
  power: 128,
  fanRpm: 1120,
  vramTotal: 12,
  vramUsed: 7.1,
  driverVersion: '555.99',
  cudaVersion: '12.4',
  computeCapability: '8.9',
};

export const ram = {
  total: 32,
  used: 19.6,
};

export const engineMode = 'Balanced';

export const smartSwap = true;

export const loadedModels = [
  {
    id: 'llama-3.1-70b',
    name: 'Llama 3.1 70B Instruct',
    type: 'LLM' as const,
    quantization: 'GGUF Q4_K_M',
    sizeOnDisk: '38.2 GB',
    vramRequired: 4.8,
    vramUsed: 4.8,
    status: 'running' as const,
    uptime: '00:12:47',
    role: 'Conversation',
    icon: 'MessageSquare',
    color: 'primary',
  },
  {
    id: 'sdxl-lightning',
    name: 'SDXL Lightning',
    type: 'Image' as const,
    quantization: 'FP16',
    sizeOnDisk: '6.9 GB',
    vramRequired: 2.3,
    vramUsed: 2.3,
    status: 'running' as const,
    uptime: '00:08:23',
    role: 'Image Generation',
    icon: 'Image',
    color: 'secondary',
  },
  {
    id: 'kokoro-tts',
    name: 'Kokoro TTS',
    type: 'Voice' as const,
    quantization: 'INT8',
    sizeOnDisk: '0.3 GB',
    vramRequired: 0.6,
    vramUsed: 0.0,
    status: 'idle' as const,
    uptime: '00:00:00',
    role: 'Text to Speech',
    icon: 'Mic',
    color: 'tertiary',
  },
];

export const allModels = [
  ...loadedModels,
  {
    id: 'mistral-7b',
    name: 'Mistral 7B Instruct v0.3',
    type: 'LLM' as const,
    quantization: 'GGUF Q4_K_M',
    sizeOnDisk: '4.4 GB',
    vramRequired: 4.8,
    vramUsed: 0,
    status: 'available' as const,
    uptime: '',
    role: 'General Chat',
    icon: 'MessageSquare',
    color: 'primary',
  },
  {
    id: 'phi-3-mini',
    name: 'Phi-3 Mini 4K Instruct',
    type: 'LLM' as const,
    quantization: 'GGUF Q4_K_M',
    sizeOnDisk: '2.3 GB',
    vramRequired: 2.6,
    vramUsed: 0,
    status: 'available' as const,
    uptime: '',
    role: 'Fast Chat',
    icon: 'MessageSquare',
    color: 'primary',
  },
  {
    id: 'flux-schnell',
    name: 'FLUX.1 Schnell',
    type: 'Image' as const,
    quantization: 'FP8',
    sizeOnDisk: '11.2 GB',
    vramRequired: 8.5,
    vramUsed: 0,
    status: 'available' as const,
    uptime: '',
    role: 'Image Generation',
    icon: 'Image',
    color: 'secondary',
  },
  {
    id: 'whisper-large-v3',
    name: 'Whisper Large v3',
    type: 'Voice' as const,
    quantization: 'GGML Q5_1',
    sizeOnDisk: '2.9 GB',
    vramRequired: 3.2,
    vramUsed: 0,
    status: 'available' as const,
    uptime: '',
    role: 'Speech to Text',
    icon: 'Mic',
    color: 'tertiary',
  },
];

export const chatMessages = [
  {
    id: '1',
    role: 'user' as const,
    content: 'Can you write a Python script that monitors a folder for new files and processes them?',
    timestamp: '09:23 AM',
  },
  {
    id: '2',
    role: 'assistant' as const,
    content: "I'll create a Python script that uses the `watchdog` library to monitor a folder for new files and process them.\n\n```python\nimport time\nimport logging\nfrom pathlib import Path\nfrom watchdog.observers import Observer\nfrom watchdog.events import FileSystemEventHandler\n\nclass FileProcessor(FileSystemEventHandler):\n    def __init__(self, process_func):\n        self.process_func = process_func\n    \n    def on_created(self, event):\n        if not event.is_directory:\n            logging.info(f\"New file detected: {event.src_path}\")\n            self.process_func(event.src_path)\n\ndef process_file(file_path):\n    # Your custom processing logic here\n    print(f\"Processing: {file_path}\")\n    # Example: convert, resize, analyze, etc.\n\nif __name__ == \"__main__\":\n    WATCH_FOLDER = Path(\"./watch_folder\")\n    WATCH_FOLDER.mkdir(exist_ok=True)\n    \n    logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(message)s')\n    \n    event_handler = FileProcessor(process_file)\n    observer = Observer()\n    observer.schedule(event_handler, str(WATCH_FOLDER), recursive=False)\n    observer.start()\n    \n    try:\n        while True:\n            time.sleep(1)\n    except KeyboardInterrupt:\n        observer.stop()\n    observer.join()\n```\n\n**Requirements:**\n```bash\npip install watchdog\n```\n\n**Usage:**\n1. Save as `folder_monitor.py`\n2. Run: `python folder_monitor.py`\n3. Drop files into `./watch_folder/` to trigger processing\n\nThe script will run continuously until you press Ctrl+C. You can customize the `process_file` function to do whatever you need—image conversion, data extraction, file organization, etc.",
    timestamp: '09:24 AM',
  },
  {
    id: '3',
    role: 'user' as const,
    content: 'What if I want to process only specific file types, like only images?',
    timestamp: '09:26 AM',
  },
  {
    id: '4',
    role: 'assistant' as const,
    content: 'You can filter by file extension in the `on_created` method. Here\'s an updated version that only processes images:',
    timestamp: '09:27 AM',
  },
  {
    id: '5',
    role: 'assistant' as const,
    content: '...',
    timestamp: '09:28 AM',
    isThinking: true,
  },
];

export const chatHistory = [
  { id: '1', title: 'Python folder monitoring script', timestamp: 'Today, 09:23 AM', messageCount: 5 },
  { id: '2', title: 'React component architecture', timestamp: 'Yesterday, 02:14 PM', messageCount: 12 },
  { id: '3', title: 'Docker compose for microservices', timestamp: 'Aug 25, 10:30 AM', messageCount: 8 },
  { id: '4', title: 'SQL query optimization tips', timestamp: 'Aug 24, 04:52 PM', messageCount: 6 },
  { id: '5', title: 'TypeScript generics deep dive', timestamp: 'Aug 23, 11:15 AM', messageCount: 15 },
];

export const recentGenerations = [
  { id: '1', thumbnail: 'linear-gradient(135deg, #667eea 0%, #764ba2 100%)', prompt: 'Cyberpunk cityscape at night, neon lights...', timestamp: 'Today, 08:45 AM' },
  { id: '2', thumbnail: 'linear-gradient(135deg, #f093fb 0%, #f5576c 100%)', prompt: 'Portrait of a robotic owl, intricate details...', timestamp: 'Today, 08:32 AM' },
  { id: '3', thumbnail: 'linear-gradient(135deg, #4facfe 0%, #00f2fe 100%)', prompt: 'Floating islands with waterfalls, fantasy art...', timestamp: 'Yesterday, 06:15 PM' },
  { id: '4', thumbnail: 'linear-gradient(135deg, #43e97b 0%, #38f9d7 100%)', prompt: 'Minimalist geometric composition, pastel colors...', timestamp: 'Yesterday, 05:42 PM' },
  { id: '5', thumbnail: 'linear-gradient(135deg, #fa709a 0%, #fee140 100%)', prompt: 'Vintage film camera on wooden table, soft lighting...', timestamp: 'Aug 25, 02:18 PM' },
  { id: '6', thumbnail: 'linear-gradient(135deg, #a8edea 0%, #fed6e3 100%)', prompt: 'Abstract fluid art, gold and white marble...', timestamp: 'Aug 25, 01:55 PM' },
];

export const voicePresets = [
  { id: 'af_sarah', name: 'Sarah (US Female)', lang: 'en-US', preview: true },
  { id: 'am_adam', name: 'Adam (US Male)', lang: 'en-US', preview: true },
  { id: 'bf_emma', name: 'Emma (UK Female)', lang: 'en-GB', preview: true },
  { id: 'bm_george', name: 'George (UK Male)', lang: 'en-GB', preview: true },
  { id: 'af_bella', name: 'Bella (US Female)', lang: 'en-US', preview: true },
  { id: 'am_michael', name: 'Michael (US Male)', lang: 'en-US', preview: true },
];

export const recentSynthesis = [
  { id: '1', text: 'Welcome to Gabriel, your local AI studio...', duration: '3.2s', timestamp: 'Today, 08:30 AM' },
  { id: '2', text: 'The quick brown fox jumps over the lazy dog...', duration: '2.1s', timestamp: 'Today, 08:15 AM' },
  { id: '3', text: 'System notification: Model swap completed successfully...', duration: '1.8s', timestamp: 'Yesterday, 04:22 PM' },
  { id: '4', text: 'Error: Unable to connect to the inference engine...', duration: '2.5s', timestamp: 'Yesterday, 03:45 PM' },
  { id: '5', text: 'Processing complete. Output saved to workspace...', duration: '1.9s', timestamp: 'Aug 25, 10:12 AM' },
];

export const workflows = [
  {
    id: '1',
    name: 'Voice Note → Transcript → Summary',
    nodes: [
      { id: '1', label: 'Voice Input', type: 'Voice' as const },
      { id: '2', label: 'Whisper STT', type: 'Voice' as const },
      { id: '3', label: 'LLM Summary', type: 'LLM' as const },
    ],
    status: 'Active' as const,
    lastRun: 'Today, 08:45 AM',
    runCount: 24,
  },
  {
    id: '2',
    name: 'Prompt → Image → Upscale',
    nodes: [
      { id: '1', label: 'Text Prompt', type: 'LLM' as const },
      { id: '2', label: 'SDXL Lightning', type: 'Image' as const },
      { id: '3', label: 'ESRGAN 4x', type: 'Image' as const },
    ],
    status: 'Active' as const,
    lastRun: 'Yesterday, 03:20 PM',
    runCount: 18,
  },
  {
    id: '3',
    name: 'RAG Document QA Pipeline',
    nodes: [
      { id: '1', label: 'Document Ingest', type: 'LLM' as const },
      { id: '2', label: 'Embeddings', type: 'LLM' as const },
      { id: '3', label: 'Vector Search', type: 'LLM' as const },
      { id: '4', label: 'LLM Answer', type: 'LLM' as const },
    ],
    status: 'Draft' as const,
    lastRun: 'Aug 25, 11:00 AM',
    runCount: 7,
  },
  {
    id: '4',
    name: 'Batch Image Generation',
    nodes: [
      { id: '1', label: 'Prompt List', type: 'LLM' as const },
      { id: '2', label: 'SDXL Batch', type: 'Image' as const },
      { id: '3', label: 'Auto-select Best', type: 'LLM' as const },
    ],
    status: 'Scheduled' as const,
    lastRun: 'Aug 24, 08:00 PM',
    runCount: 3,
  },
];

export const recentRuns = [
  { id: '1', workflow: 'Voice Note → Transcript → Summary', status: 'success' as const, timestamp: 'Today, 08:45 AM', duration: '12.3s' },
  { id: '2', workflow: 'Prompt → Image → Upscale', status: 'success' as const, timestamp: 'Yesterday, 03:20 PM', duration: '28.7s' },
  { id: '3', workflow: 'RAG Document QA Pipeline', status: 'running' as const, timestamp: 'Today, 09:15 AM', duration: '45.2s' },
  { id: '4', workflow: 'Batch Image Generation', status: 'failed' as const, timestamp: 'Aug 25, 11:30 AM', duration: '8.1s' },
];

export const workflowTemplates = [
  { id: '1', name: 'RAG Pipeline', description: 'Document ingestion → Embedding → Retrieval → Generation', icon: 'Database' },
  { id: '2', name: 'Voice Assistant Loop', description: 'Wake word → STT → LLM → TTS → Playback', icon: 'Mic' },
  { id: '3', name: 'Batch Image Generation', description: 'Prompt list → Parallel generation → Selection → Export', icon: 'Image' },
];

export const processTableData = [
  { name: 'Llama 3.1 70B Instruct', type: 'LLM' as const, status: 'running' as const, vram: '4.8 GB', ram: '8.2 GB', uptime: '00:12:47', pid: 4821 },
  { name: 'SDXL Lightning', type: 'Image' as const, status: 'running' as const, vram: '2.3 GB', ram: '1.1 GB', uptime: '00:08:23', pid: 4903 },
  { name: 'Kokoro TTS', type: 'Voice' as const, status: 'idle' as const, vram: '0.0 GB', ram: '0.3 GB', uptime: '00:00:00', pid: 4956 },
  { name: 'Embedding Service', type: 'LLM' as const, status: 'running' as const, vram: '0.5 GB', ram: '0.8 GB', uptime: '02:34:12', pid: 4789 },
];

export const gpuHistory = {
  temp: [45, 46, 47, 48, 48, 47, 46, 48, 48, 47, 46, 45, 44, 45, 46, 47, 48, 48, 47, 46],
  power: [110, 115, 120, 125, 128, 126, 124, 128, 130, 128, 125, 122, 120, 118, 115, 120, 125, 128, 126, 124],
  fan: [980, 1000, 1020, 1050, 1080, 1100, 1120, 1120, 1100, 1080, 1060, 1040, 1020, 1000, 980, 1000, 1050, 1100, 1120, 1120],
};

export const engineStatus = [
  { name: 'Model Manager', status: 'Running' as const },
  { name: 'Bandwidth Governor', status: 'Running' as const },
  { name: 'Memory Manager', status: 'Running' as const },
  { name: 'VRAM Pager', status: 'Running' as const },
];

export const systemAlerts = [
  { id: '1', type: 'info' as const, message: 'Model paged to RAM: llama2-13b', timestamp: 'Today, 08:23 AM', icon: 'HardDrive' },
  { id: '2', type: 'success' as const, message: 'GPU temp normalized to 48°C', timestamp: 'Today, 08:15 AM', icon: 'Thermometer' },
  { id: '3', type: 'warning' as const, message: 'VRAM usage above 80% watermark threshold', timestamp: 'Today, 07:45 AM', icon: 'AlertTriangle' },
  { id: '4', type: 'info' as const, message: 'Engine governor policy updated', timestamp: 'Yesterday, 06:30 PM', icon: 'Cpu' },
  { id: '5', type: 'success' as const, message: 'Model SDXL Lightning loaded successfully', timestamp: 'Yesterday, 05:55 PM', icon: 'CheckCircle' },
];

export const storageBreakdown = [
  { type: 'LLM', used: 89, color: 'primary' },
  { type: 'Image', used: 32, color: 'secondary' },
  { type: 'Voice', used: 8, color: 'tertiary' },
  { type: 'Other', used: 13, color: 'gray' },
];

export const settingsSections = [
  { id: 'general', label: 'General', icon: 'Settings' },
  { id: 'appearance', label: 'Appearance', icon: 'Palette' },
  { id: 'governor', label: 'Engine Governor', icon: 'Cpu' },
  { id: 'shortcuts', label: 'Keyboard Shortcuts', icon: 'Keyboard' },
];

export const quickActions = [
  { id: '1', icon: 'MessageSquare', label: 'Chat with LLMs', description: 'Have conversations with local models', route: '/chat', color: 'primary' as const },
  { id: '2', icon: 'Image', label: 'Generate Image', description: 'Create images with stable diffusion', route: '/image', color: 'secondary' as const },
  { id: '3', icon: 'Mic', label: 'Text to Speech', description: 'Generate natural speech locally', route: '/voice', color: 'tertiary' as const },
  { id: '4', icon: 'Box', label: 'Browse Models', description: 'Manage & load local AI models', route: '/models', color: 'primary' as const },
];

export const rightPanelQuickActions = [
  { id: '1', label: 'Load Model', icon: 'Download' },
  { id: '2', label: 'System Monitor', icon: 'Activity' },
  { id: '3', label: 'Engine Settings', icon: 'Settings' },
  { id: '4', label: 'View Logs', icon: 'FileText' },
];

export const modelRegistryActions = [
  { id: '1', label: 'Browse Hugging Face', icon: 'Globe', description: 'Search and download from HF Hub' },
  { id: '2', label: 'Import local GGUF', icon: 'Upload', description: 'Add a local GGUF model file' },
  { id: '3', label: 'Import local safetensors', icon: 'Upload', description: 'Add a local safetensors model' },
];