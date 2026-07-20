import React, { useState, useEffect } from "react";
import { motion } from "framer-motion";

interface SmartMeshChatProps {
    visible: boolean;
    onComplete: () => void;
    context?: any;
}

export const SmartMeshChat: React.FC<SmartMeshChatProps> = ({ visible, onComplete, context }) => {
    const [messages, setMessages] = useState<any[]>([]);

    useEffect(() => {
        if (visible) {
            // Determine scenario based on context
            // Default (Generic Intent)
            let initialMessages: any[] = [
                { id: 1, sender: 'User A', text: 'Hi B, can you send me the logo file?', type: 'text' },
                { id: 2, sender: 'User B', text: 'Sure, wait a sec.', type: 'text' }
            ];

            // Specific Scenarios
            if (context?.user === 'User_C') { // ESP32 Case
                initialMessages = [
                    { id: 1, sender: 'User A', text: 'Hi, I have standard ESP32-WROOM-32 modules available.', type: 'text' },
                    { id: 2, sender: 'User_C', text: 'great, how much for 10 units?', type: 'text' }
                ];
            } else if (context?.user === 'BEYOND_US NFT') { // NFT Case
                initialMessages = [
                    { id: 1, sender: 'User A', text: 'Love the new collection! Is #42 available?', type: 'text' },
                    { id: 2, sender: 'User_X', text: 'Yes, setting up an escrow swap now.', type: 'text' }
                ];
            }

            setMessages(initialMessages);

            // Simulate Flow
            setTimeout(() => {
                setMessages(prev => [...prev, {
                    id: 3,
                    type: 'ai-separator',
                    text: 'AI Node Monitoring Transaction'
                }]);
            }, 1000);

            setTimeout(() => {
                setMessages(prev => [...prev, {
                    id: 4,
                    sender: 'AI',
                    type: 'ai-msg',
                    text: context?.user === 'User_C'
                        ? 'Detected bulk component sale. Checking market rates... Median is 8 AVAX.'
                        : 'I detected a purchase intent. Escrow initialized for safety.'
                }]);
            }, 2500);

            setTimeout(() => {
                setMessages(prev => [...prev, {
                    id: 5,
                    type: 'widget',
                    data: {
                        item: context?.content || 'Logo_Design_v1.zip',
                        price: context?.fee || '50 AVAX'
                    }
                }]);
            }, 4000);
        }
    }, [visible, context]);

    if (!visible) return null;

    return (
        <motion.div
            className="absolute inset-0 bg-nobody-dark z-40 flex flex-col"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
        >
            {/* Header */}
            <div className="h-10 border-b border-slate-200 flex items-center justify-between px-4 bg-white">
                <div className="text-xs text-nobody-mint font-semibold tracking-wide">
                    Mesh: 4ms | AI-Node: Active (x402)
                </div>
                <div className="flex gap-2 items-center">
                    <button onClick={onComplete} className="text-[11px] text-slate-400 hover:text-slate-900 mr-4 transition-colors">
                        End Chat
                    </button>
                    <div className="w-2 h-2 rounded-full bg-nobody-mint animate-pulse" />
                    <div className="w-2 h-2 rounded-full bg-nobody-violet animate-pulse delay-75" />
                </div>
            </div>

            {/* Chat Area */}
            <div className="flex-1 overflow-y-auto p-4 space-y-4">
                {messages.map((msg) => (
                    <div key={msg.id} className="flex flex-col animate-fadeIn">

                        {msg.type === 'ai-separator' && (
                            <div className="flex items-center gap-2 my-4 text-nobody-violet text-[11px] font-semibold justify-center opacity-80">
                                <div className="h-px w-10 bg-nobody-violet/30" />
                                {msg.text}
                                <div className="h-px w-10 bg-nobody-violet/30" />
                            </div>
                        )}

                        {msg.type === 'text' && (
                            <div className={`flex flex-col max-w-[70%] ${msg.sender === 'User A' ? 'self-end items-end' : 'self-start items-start'}`}>
                                <div className="text-[11px] text-slate-400 mb-1">
                                    {msg.sender === 'User A' ? 'You' : msg.sender}
                                </div>
                                <div className={`px-3 py-2 rounded-2xl ${msg.sender === 'User A' ? 'bg-nobody-violet-soft text-nobody-ink' : 'bg-slate-100 text-slate-700'}`}>
                                    {msg.text}
                                </div>
                            </div>
                        )}

                        {msg.type === 'ai-msg' && (
                            <div className="self-center w-[80%] bg-nobody-mint-soft/40 border border-nobody-mint/20 rounded-2xl p-3 my-2 text-xs">
                                <strong className="text-nobody-mint">AI: </strong>
                                <span className="text-slate-600">{msg.text}</span>
                            </div>
                        )}

                        {msg.type === 'widget' && (
                            <div className="self-center w-[80%] my-2">
                                <EscrowWidget data={msg.data} />
                            </div>
                        )}
                    </div>
                ))}
            </div>

            {/* Input Area */}
            <div className="h-14 border-t border-slate-200 bg-white flex items-center px-4 gap-3">
                <span className="text-slate-300">/</span>
                <input
                    type="text"
                    placeholder="Type AI mandate or chat..."
                    className="flex-1 bg-transparent outline-none text-sm text-slate-900 placeholder-slate-400"
                />
            </div>
        </motion.div>
    );
};

const EscrowWidget = ({ data }: { data: any }) => (
    <div className="rounded-2xl border border-slate-200 bg-white shadow-card overflow-hidden">
        <div className="bg-slate-50 px-3 py-2 flex justify-between items-center border-b border-slate-100">
            <span className="text-[11px] font-semibold text-slate-900">Smart Escrow Widget</span>
        </div>
        <div className="p-4 space-y-2">
            <div className="flex justify-between text-xs text-slate-500">
                <span>Item:</span>
                <span className="text-slate-900 font-semibold">{data.item}</span>
            </div>
            <div className="flex justify-between text-xs text-slate-500">
                <span>Price:</span>
                <span className="text-nobody-mint font-semibold">{data.price}</span>
            </div>

            <div className="flex gap-2 mt-3 pt-2 border-t border-slate-100">
                <button className="flex-1 bg-nobody-mint-soft text-nobody-mint text-[11px] font-semibold rounded-lg py-2 hover:bg-nobody-mint hover:text-white transition-colors">
                    User A: Deposit
                </button>
                <button className="flex-1 bg-slate-100 text-slate-500 text-[11px] font-semibold rounded-lg py-2 hover:bg-slate-200 hover:text-slate-900 transition-colors">
                    User B: Send
                </button>
            </div>
        </div>
    </div>
);
