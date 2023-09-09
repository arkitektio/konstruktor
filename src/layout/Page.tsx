import { motion } from "framer-motion";
import { cn } from "../utils";

export const Page = ({ children, className }: {children: React.ReactNode, className?: string}) => {
    return (
        <motion.main className={cn("w-full h-full flex flex-col", className)}  animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
    transition={{ duration: 2 }}
        >
        {children}
        </motion.main>
    );
    }