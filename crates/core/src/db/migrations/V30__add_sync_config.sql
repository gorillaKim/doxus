-- Migration: Add sync_policy_json to projects table
-- This allows storing complex synchronization policies (Realtime, Interval, OnFocus, Manual)
-- for each project based on its source capabilities.

ALTER TABLE projects ADD COLUMN sync_policy_json TEXT;
