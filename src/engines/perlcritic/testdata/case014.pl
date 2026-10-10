use feature 'try';
try { die } catch ($e) { warn $e } finally { 1 }
