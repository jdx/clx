pub struct Progress {
    total: usize,
    cur: usize,
}

impl Progress {
    pub fn new(total: usize) -> Self {
        Self { total, cur: 0 }
    }

    pub fn update(&mut self, cur: usize) {
        self.cur = cur;
    }

    pub fn view(&self) -> String {
        format!("{} / {}", self.cur, self.total)
    }
}
